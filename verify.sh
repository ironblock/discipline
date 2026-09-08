#!/usr/bin/env bash
#
# The gate. Everything that must be true of this repository is checked here,
# and CI runs exactly this script.
#
#   verify.sh                 run every check
#   verify.sh --only CHECK    run one check (repeatable)
#   verify.sh --only test --scope SPEC   narrow the test check (selftest only)
#   verify.sh --list          name the checks, in order
#   verify.sh --selftest      prove the gate goes red on seeded faults
#   verify.sh --selftest --shard K/N    run this job's share of the faults
#   verify.sh --selftest --census PATH  write what this run ran, for the sum
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

readonly CHECKS=(fmt clippy test library results recompute regimen metadata hygiene pages ci history injections resolver parity)

# The forbidden classes the genesis brief names by hand. Pinning them here
# means a pattern row cannot be deleted along with its seeded class and leave
# the selftest still reporting success.
# Every class, not only the ones the brief names: otherwise a pattern and its
# seeded class can be deleted together and the selftest still reports success.
readonly REQUIRED_HYGIENE_CLASSES=(
  private-ipv4 internal-hostname personal-home-path windows-user-path
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
check_recompute() { python3 scripts/check-recompute.py; }

check_metadata() { python3 scripts/check-repo-metadata.py; }

check_hygiene() { bash scripts/hygiene.sh; }

# The site published to gh-pages is static, and this is what makes that a gate
# rather than a promise: no subresource from another origin, no network call,
# no form, no credential shapes.
check_pages() {
  bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree pages
}

# The wiring between these checks and the CI that runs them. CI can go green
# while running almost nothing -- a check owned by no workflow, a job the gate
# does not depend on, a path filter that turns a skip into a pass.
check_ci() { python3 scripts/check-ci-coverage.py; }

# Commit messages, and a pull request's title and body, against the same
# pattern table the file gate uses. A file carrying a forbidden shape can be
# fixed with a commit; a commit message carrying one is permanent.
check_history() { python3 scripts/check-history.py; }

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

# --- the shard ------------------------------------------------------------
#
# `--selftest --shard K/N` runs every Nth fault starting at the Kth, so N jobs
# between them run each fault exactly once. Round-robin rather than blocks:
# the Rust-class faults are the expensive ones and they are declared in runs,
# so a block split would put nearly all of them in one shard.
#
# This is NOT fault selection. Nothing here decides that a fault need not run;
# it decides which JOB runs it, and scripts/check-selftest-census.py proves
# after the fact that the shards between them ran every one. Selecting faults
# by what changed is the thing this repository refuses, and the difference is
# that a shard's absence is a failure rather than a silence: a missing census
# is a missing shard, and the aggregate refuses.
#
# 0 means unsharded -- one job runs the lot, which is what a contributor gets.
SELFTEST_SHARD=0
SELFTEST_SHARDS=1
SELFTEST_UNITS=0
SELFTEST_RAN=()
SELFTEST_CENSUS=""

# Whether the next counted fault belongs to this shard, counting it either
# way. Every counted fault calls this exactly once, in declaration order, so
# the ordinals are the same in every shard and the union of the shards is the
# whole list -- which is the claim the census script checks rather than trusts.
in_shard() {
  SELFTEST_UNITS=$(( SELFTEST_UNITS + 1 ))
  if [ "$SELFTEST_SHARD" -eq 0 ] ||
     [ "$(( (SELFTEST_UNITS - 1) % SELFTEST_SHARDS + 1 ))" -eq "$SELFTEST_SHARD" ]; then
    SELFTEST_RAN+=("$SELFTEST_UNITS")
    return 0
  fi
  return 1
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
seeded_case() {
  local label="$1" check="$2" inject="$3" expect="$4" scope="${5-}"
  local box="$SELFTEST_BOX"
  local started="$SECONDS"
  # Recorded before the shard is consulted. This list answers "has every check
  # been seen red", which is a question about what the gate DECLARES, and the
  # answer must not depend on which shard is asking.
  SEEDED_CHECKS+=("$check")
  in_shard || return 0
  SELFTEST_CASES=$(( SELFTEST_CASES + 1 ))

  # A `test` case says which tests it needs; anything else says nothing,
  # because `--scope` narrows that one check and verify.sh refuses it
  # elsewhere. Both halves are reported here rather than left to become a
  # confusing exit 2 from inside the box, and scripts/check-fault-manifest.py
  # refuses the same two states before a run ever starts.
  local -a scoped=()
  if [ "$check" = "test" ]; then
    if [ -z "$scope" ]; then
      printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- NO TEST SCOPE DECLARED\n' \
        "$(( SECONDS - started ))" "$check" "$label"
      SELFTEST_BROKEN+=("${label}: a test case must name the tests it needs")
      return
    fi
    scoped=(--scope "$scope")
  elif [ -n "$scope" ]; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- A SCOPE ON A CHECK THAT TAKES NONE\n' \
      "$(( SECONDS - started ))" "$check" "$label"
    SELFTEST_BROKEN+=("${label}: only the test check takes a scope")
    return
  fi
  # One log per case, kept for the run, because the box itself is overwritten
  # by the case after this one and a failure is read after the fact.
  local log="${SELFTEST_LOGS}/$(printf '%03d' "$SELFTEST_CASES").log"

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
    return
  fi

  # An injection is a `sed` or a `printf` against a file it names. Rename the
  # file, or reshape the line the pattern matches, and the injection silently
  # becomes a no-op -- the case then runs a clean tree, goes green, and is
  # reported as a gate that did not fire. That is the right verdict for the
  # wrong reason, and it costs a debugging session every time. Fingerprint the
  # sandbox instead, and say which of the two actually happened.
  local state_before state_after
  state_before="$(sandbox_state "$box")"
  ( cd "$box" && "$inject" )
  state_after="$(sandbox_state "$box")"
  case "${state_before}${state_after}" in
    *"${STATE_UNREADABLE}"*)
      printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE SANDBOX COULD NOT BE READ\n' \
        "$(( SECONDS - started ))" "$check" "$label"
      SELFTEST_BROKEN+=("${label}: the sandbox's state could not be read")
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

  if [ "$state_before" = "$state_after" ]; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE INJECTION CHANGED NOTHING\n' \
      "$(( SECONDS - started ))" "$check" "$label"
    SELFTEST_BROKEN+=("${label}: ${inject} changed nothing, so the case proves nothing")
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
  sed -i \
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
  sed -i 's|^    let joined = value.join("\\n");$|    let joined = value.first().cloned().unwrap_or_default();|' \
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
  sed -i 's|^    if depth > MAX_DEPTH {$|    if false {|' diet/src/formats/record/mod.rs
}

inject_record_substrate_optional() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '    let mut substrate_members = take_object(members, of, "substrate")?;'
new = """    let mut substrate_members = take_object(members, of, "substrate").unwrap_or_else(|_| {
        BTreeMap::from([
            ("name".to_owned(), Value::String("unknown".to_owned())),
            ("model".to_owned(), Value::String("unknown".to_owned())),
            ("quantization".to_owned(), Value::String("unknown".to_owned())),
            (
                "sampler".to_owned(),
                Value::Object(BTreeMap::from([(
                    "seed".to_owned(),
                    Value::Integer(0),
                )])),
            ),
            ("reasoning".to_owned(), Value::String("off".to_owned())),
            ("hardware".to_owned(), Value::String("unknown".to_owned())),
        ])
    });"""
assert old in source
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
  sed -i 's|^        if grounded == 0 {$|        if false {|' diet/src/capture/grounded.rs
}

inject_grounded_floor_inert() {
  sed -i 's|^    let outcome = if score.meets(floor) {$|    let outcome = if true {|' \
    diet/src/capture/grounded.rs
}

# The judgment exemption removed. Grounding a plan is a category error, and a
# gate that does it rejects legitimate content -- the failure that made the
# scoping a ruling rather than an implementation detail.
inject_grounded_gates_judgment() {
  sed -i 's|^        !matches!(self, Self::Judgment)$|        let _ = self; true|' \
    diet/src/capture/grounded.rs
}

# A measurement handed back without its instrument having been seen fail. The
# 1.000 that meant nothing was a real score, computed by real code, on a probe
# where fabrication was structurally impossible.
inject_grounded_undemonstrated() {
  sed -i 's|^        if demonstrated_failure.outcome != LaneOutcome::Rejected {$|        if false {|' \
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
  sed -i '/^pub mod object;$/d' diet/src/lib.rs
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

# The acceptance case where THE BUILD IS THE GATE: a field-kind variant added
# without wiring it. Every exhaustive match over FieldKind stops compiling,
# which is the whole reason the predicate is an enum.
inject_field_kind_variant() {
  sed -i 's|^    Stuck,$|    Stuck,\n    /// Seeded: a variant nothing covers.\n    Seeded,|' \
    diet/src/formats/interview.rs
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
inject_record_negative_zero_decimal() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/record/grammar.pest")
source = path.read_text(encoding="utf-8")
old = """decimal = @{ ("-" ~ negative_decimal) | (int_part ~ "." ~ ASCII_DIGIT+) }"""
new = """decimal = @{ "-"? ~ int_part ~ "." ~ ASCII_DIGIT+ }"""
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
  sed -i 's|^const CLOSING_LANE: &str = "main";$|const CLOSING_LANE: \&str = "tangent-closure";|' \
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
  sed -i 's|^            Self::Parked => "parked",$|            Self::Parked => "retired",|' \
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
  sed -i 's|^const EXIT_USAGE: u8 = 2;$|const EXIT_USAGE: u8 = 0;|' diet/src/bin/diet.rs
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
  sed -i 's|^    println!("{rendered}");$||' diet/src/bin/diet.rs
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
  sed -i 's|^        if let Some(held) = self.by_content.get(&key).cloned() {$|        if let Some(held) = None::<EntryId> {|' \
    diet/src/object.rs
}

inject_results() {
  cp -r tests/fixtures/results-bad/2026-01-09-unbacked-number results/
}

# A run.jsonl whose start row has lost its substrate. The report is fine; the
# record is not a session record, and only diet says so -- the linter must
# relay that verdict and reach none of its own.
inject_results_no_substrate() {
  cp -r results/_template results/2026-01-30-no-substrate
  python3 - <<'EOF'
import json, pathlib

path = pathlib.Path("results/2026-01-30-no-substrate/run.jsonl")
lines = path.read_text(encoding="utf-8").split("\n")
row = json.loads(lines[0])
assert row["record"] == "start"
del row["regime"]["substrate"]
lines[0] = json.dumps(row, separators=(",", ":"))
path.write_text("\n".join(lines), encoding="utf-8")
EOF
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
  sed -i 's|^    pinned = os.environ.get("DIET_BIN")$|    pinned = None|' \
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
  sed -i 's/"name": "claim"/"name": "claim-renamed"/' .github/labels.json
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
inject_regimen_float_rule_widened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/regimen/grammar.pest")
source = path.read_text(encoding="utf-8")
old = 'float     = @{ ("-" ~ negative_float) | (int_part ~ "." ~ ASCII_DIGIT+) }\n'
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
  sed -i 's/^kind = "reproducible-by-config"$/kind = "historical-observation"/' \
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

# Results present and none of them recomputed, with the template excluded from
# the count. A check of nothing is not a pass, applied to results -- and the
# directory this leaves behind is entirely LEGAL, which is the point: the
# census goes red on the shape of the tree, not on a defect in the directory.
inject_recompute_only_the_template_recomputes() {
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
  sed -i '/^hygiene\t/d' .github/check-owners.tsv
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

for path in (pathlib.Path("verify.sh"), manifest):
    source = path.read_text(encoding="utf-8")
    if source.count(old) != 1:
        raise SystemExit(f"{path}: the signature to stale appears {source.count(old)} times")
    path.write_text(source.replace(old, NEW), encoding="utf-8")
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
  sed -i '/^    branches: \[main\]$/d' \
    .github/workflows/pages.yml .github/workflows/repo-metadata.yml
}

# One character of the trunk's name, in the gating workflow only. `mian` is a
# branch nobody pushes to, so the push trigger fires for nothing and the gate
# watches a branch that does not exist -- and every pull request is still
# green, because the pull-request trigger is untouched. Nothing in this file
# knows what the trunk is called; the other workflows that name it do, and
# they are what catches this.
inject_ci_trunk_typo() {
  sed -i 's|^    branches: \[main\]$|    branches: [mian]|' .github/workflows/verify.yml
}

# A gating workflow that narrows the test check to part of the suite. The job
# is green, the run is faster, and most of the tests did not happen.
inject_ci_scoped_test() {
  sed -i 's|^\( *\)\./verify\.sh "\${args\[@\]}"$|\1./verify.sh "${args[@]}" --scope lib|' \
    .github/workflows/pkg-diet.yml
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
        if row.score < bottom_score {
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
old = "    let pooled = f64::midpoint(positive.variance, negative.variance).sqrt();\n"
new = "    let pooled = positive.variance.sqrt();\n"
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
  sed -i 's|^    let kept = !report.kept().is_empty();$|    let kept = true;|' \
    diet/src/capture/tools.rs
}
# The reminder that never comes round. Every model eventually stops recording;
# a cadence that cannot fire turns the forget rate this lane exists to survive
# into a silence nobody counts.
inject_tools_reminder_silent() {
  sed -i 's|^        if self.since < self.cadence.interval() {$|        if true {|' \
    diet/src/capture/tools.rs
}
# A harness tool call accepted as a capture. Another system's tool output then
# becomes a fact about this session, written with capture authority, and the
# provenance says the model recorded it.
inject_tools_foreign_call() {
  sed -i 's|^        return Err(ToolError::NotACaptureTool(tool.clone()));$|        return Ok(Effect::default());|' \
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
  sed -i 's|^            Ordering::Greater => return,$|            Ordering::Greater => \&mut self.source,|' \
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
  sed -i 's|^            target: entry.clone(),$|            target: EntryId::new("somebody/else").map_err(ToolError::BadEntry)?,|' \
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
  sed -i 's|^                kind: AskKind::Sweep,$|                kind: AskKind::Reminder,|' \
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
  sed -i 's|^            reason: text_argument(&args, "reason"),$|            reason: String::new(),|' \
    diet/src/capture/tools.rs
}
# A reminder that drops what the router put off. The deferral then reaches only
# the post-drive sweep, and the cadence half of the join with the router is
# dead while every test stays green.
inject_tools_reminder_deferral_dropped() {
  sed -i 's|^            about: self.deferred.get(&turn).cloned(),$|            about: None,|' \
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
  sed -i 's|"of":\["done","not_this","partial","superseded"\]|"of":["abandoned","done","not_this","partial","superseded"]|' \
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
  sed -i 's|^            self.silent.remove(&turn);$||' diet/src/capture/tools.rs
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

  while IFS=$'\t' read -r label flags regex || [ -n "${label:-}" ]; do
    case "$label" in ''|\#*) continue ;; esac
    [ -n "${regex:-}" ] || continue
    # Before the shard: whether the brief's required classes are in the table
    # is a question about the table, not about this job's share of it.
    defined+=("$label")
    in_shard || continue

    dir="${seed}/${label}"
    if [ ! -d "$dir" ]; then
      printf 'UNSEEDED %s pattern %s  <-- NO CLASS TO PROVE IT AGAINST\n' "$kind" "$label"
      SELFTEST_BROKEN+=("${kind} pattern ${label} has no seeded class")
      continue
    fi

    rc=0
    out="$(bash "${ROOT}/scripts/hygiene.sh" --patterns "${ROOT}/${table}" --tree "$dir" 2>&1)" || rc=$?
    if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q "hygiene: ${label}:"; then
      printf 'RED   hygiene.sh exit %-3d  %s\n' "$rc" "$label"
    else
      printf 'GREEN hygiene.sh exit %-3d  %s  <-- PATTERN DID NOT FIRE\n' "$rc" "$label"
      SELFTEST_BROKEN+=("${kind} pattern ${label}")
    fi
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
  printf '{"before":"%s","after":"%s"}' "$fake_head" "$fake_head" \
    > "${fake}/push-empty.json"

  expect_exit "history: a faked pull request with a dirty body" 1 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-dirty.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  expect_exit "history: a faked push whose range is empty" 2 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
        GITHUB_EVENT_PATH="${fake}/push-empty.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  expect_exit "history: a faked new-branch push resolves a base" 0 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
        GITHUB_EVENT_PATH="${fake}/push-new-branch.json" \
      python3 "${fake}/repo/scripts/check-history.py"

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
  trap selftest_cleanup EXIT
  scratch; SELFTEST_TARGET="${SCRATCH}/target"
  scratch; SELFTEST_LOGS="$SCRATCH"
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
    'conformance failure\(s\)' 'test:conformance/formats::regimen'
  seeded_case "FORMATS emptied, harness covers none"  test     inject_formats_empty \
    'FORMATS is empty' 'test:conformance'
  seeded_case "decline grammar loses its end anchor"  test     inject_decline_unanchored \
    'coordinated-with-and\.txt: accepted as' 'test:conformance/formats::decline'
  seeded_case "interview drops continuation lines"    test     inject_interview_drops_continuations \
    'multi-line-continuation\.txt: parsed to' 'test:conformance/formats::interview'
  seeded_case "interview blind to truncation"         test     inject_interview_truncation_blind \
    'truncated-unterminated-fence\.txt: parsed to' 'test:conformance/formats::interview'
  seeded_case "interview discards trailing content"   test     inject_interview_discards_trailing \
    'content lost parsing' 'lib/formats::interview::tests'
  seeded_case "an event kind with no fixture"         test     inject_record_unfixtured_kind \
    'no fixture.*spurious' 'lib/formats::record::tests'
  seeded_case "record substrate made optional"        test     inject_record_substrate_optional \
    'regime-missing-substrate\.jsonl: accepted as' 'test:conformance/formats::record'
  seeded_case "a row that links to itself"            test     inject_record_self_link_allowed \
    'retry-of-itself\.jsonl: accepted as' 'test:conformance/formats::record'
  seeded_case "record nesting left unbounded"         test     inject_record_depth_unbounded \
    'deep-nesting\.jsonl: accepted as' 'test:conformance/formats::record'
  seeded_case "grounding floor made inert"            test     inject_grounded_floor_inert \
    'a lane that mostly fabricated must be rejected whole' 'lib/capture::grounded::tests'
  seeded_case "grounding gates a judgment field"      test     inject_grounded_gates_judgment \
    'the gate touched a judgment-class field' 'lib/capture'
  seeded_case "a score with no demonstrated failure"  test     inject_grounded_undemonstrated \
    'a measurement was handed back whose instrument never failed' 'lib/capture::grounded::tests'
  seeded_case "grounding loosened to recombination"   test     inject_grounded_loose_matching \
    'a sentence the source never said was scored as present in it' 'lib/capture::grounded::tests'
  seeded_case "a floor of zero"                       test     inject_grounded_zero_floor \
    'a floor of zero was accepted, and every lane meets it' 'lib/capture::grounded::tests'
  seeded_case "supersede deletes what it replaced"    test     inject_object_supersede_deletes \
    'the voided entry is still here' 'lib'
  seeded_case "the reconciler stops deduping"         test     inject_object_no_dedup \
    'the same fact, wrapped differently, is the same fact' 'lib/object'
  seeded_case "a correction that restates what it voids" test   inject_object_self_void \
    'an entry was voided by itself' 'lib/object::tests'
  seeded_case "a state change over a supersede link"  test     inject_object_state_overwrite \
    'a correction was erased by a later state change' 'lib/object::tests'
  seeded_case "a no-op patch that claims an entry"    test     inject_object_false_attribution \
    'the patch claimed entries no diff can support' 'lib/object::tests'
  seeded_case "a usage error that exits zero"         test     inject_cli_usage_exit \
    'a usage error must be distinguishable from a bad document' 'test:cli'
  seeded_case "a verb wired to the wrong format"      test     inject_cli_wrong_format \
    'a valid fixture of its own format did not read' 'test:cli'
  seeded_case "a CLI that prints no result"           test     inject_cli_silent \
    'stdout is not JSON' 'test:cli'
  seeded_case "the regime moves under a patch"        test     inject_object_regime_mutable \
    'moved the regime the object was opened under' 'lib/object::tests'
  seeded_case "dedup rebinds instead of aliasing"     test     inject_object_no_alias \
    'a lane was told the object had never heard of the id it chose' 'lib/object::tests'
  seeded_case "a turn applied in arrival order"       test     inject_object_unsorted_turn \
    'the outcome of a turn depended on the order its patches arrived in' 'lib/object::tests'
  seeded_case "a self-supersede through an alias"     test     inject_object_alias_self_void \
    'was not named as one' 'lib/object::tests'
  seeded_case "a field kind nothing covers"           test     inject_field_kind_variant \
    'non-exhaustive patterns' 'lib'
  seeded_case "a subshell read as a group"            test     inject_shell_subshell_as_group \
    'the bracketed list was not read as a subshell' 'lib'
  seeded_case "a stderr pipe read as a plain pipe"    test     inject_shell_stderr_pipe_flat \
    'did not become the duplication it abbreviates' 'lib/formats::shell::tests'
  seeded_case "an expanding word reported literal"    test     inject_shell_expansion_literal \
    'a word the shell would expand was reported literal' 'lib'
  seeded_case "an empty payload read as absent"       test     inject_record_empty_payload_dropped \
    'an empty answer is a recorded answer, not a missing one' 'lib/formats::record::tests'
  seeded_case "a tangent drop that deletes"           test     inject_tangent_drop_removes \
    'a drop evicts to the archive and never deletes' 'lib/object::tangent::tests'

  seeded_case "a parked entry that still renders"     test     inject_tangent_park_renders \
    'a parked entry still speaks for the object' 'lib/object::tangent::tests'

  seeded_case "a tangent scoped by recency"           test     inject_tangent_scope_by_recency \
    'scope is by provenance, not by recency' 'lib/object::tangent::tests'

  seeded_case "a tangent closed leaving an entry unruled" test  inject_tangent_undisposed_ignored \
    'closure is total: a tangent-born entry was left undisposed' 'lib/object::tangent::tests'

  seeded_case "a prefix claimed intact, not compared" test     inject_tangent_prefix_asserted \
    'the prefix was reported intact after the trunk moved' 'lib/object::tangent::tests'

  seeded_case "a tangent id opened twice"             test     inject_tangent_id_reused \
    'a tangent id the record already carries was opened again' 'lib/object::tangent::tests'

  seeded_case "a tangent that dates a fact at the fork" test   inject_tangent_provenance_ignores_turn \
    'a tangent dated a patch at a turn other than the one it was made at' 'lib/object::tangent::tests'

  seeded_case "a closure ruling dated at the fork"    test     inject_tangent_ruling_dated_at_fork \
    'the closure filed its ruling at a turn it did not close at' 'lib/object::tangent::tests'

  seeded_case "a closure under a coined lane"         test     inject_tangent_closing_lane_coined \
    'the closure filed its ruling under a lane the record does not already have' 'lib/object::tangent::tests'

  seeded_case "a disposition missing from the list"   test     inject_tangent_disposition_missing_from_all \
    'the dispositions a closure can rule with are not the three the record names' 'lib/object::tangent::tests'

  seeded_case "a tangent that offers a settled fact again" test inject_tangent_scope_offers_a_ruled_entry \
    'the tangent offered a fact the record had already ruled on for disposition a second time' 'lib/object::tangent::tests'

  seeded_case "a prefix that counts dead trunk rows"  test     inject_tangent_prefix_counts_dead_rows \
    'a trunk row that was already dead at the fork was counted into the prefix' 'lib/object::tangent::tests'

  seeded_case "a prefix called intact whenever it only grew" test inject_tangent_prefix_only_grew \
    'the prefix was reported unmoved after the trunk wrote a fact of its own' 'lib/object::tangent::tests'

  seeded_case "a parked entry that reads as retired"  test     inject_object_park_renders_as_retired \
    'a state renders under a name the dump does not promise' 'lib/object::tests'

  seeded_case "a park with no tangent behind it"      test     inject_object_park_needs_no_tangent \
    'an entry was parked under no tangent, so it left the live set' 'lib/object::tests'

  seeded_case "a negative zero decimal accepted"      test     inject_record_negative_zero_decimal \
    'was constructed, and the grammar would not read it back' 'lib'
  seeded_case "the producer read as the last command" test     inject_shell_producer_is_last_written \
    'produces its output with' 'lib/formats::shell::tests'
  seeded_case "an operator dropped from the table"    test     inject_shell_operator_table_row_dropped \
    'the table gained or lost an operator' 'lib/formats::shell::tests'
  seeded_case "a stripping heredoc that keeps tabs"   test     inject_shell_heredoc_strip_keeps_tabs \
    'did not strip the tabs the shell strips' 'lib/formats::shell::tests'
  seeded_case "the command word read as an operand"   test     inject_shell_command_word_is_an_operand \
    'the command word is not one of its own operands' 'lib'
  seeded_case "a regimen float rendered as a string"  test     inject_regimen_float_as_a_string \
    'a float projected as something other than a decimal' 'lib/formats::regimen::tests'
  seeded_case "a table header that opens nothing"     test     inject_regimen_table_scope_flattened \
    'its keys are not the document.s' 'lib/formats::regimen::tests'
  seeded_case "a header comment read as a table"      test     inject_regimen_header_comment_is_a_table \
    'a comment after a table header is not a table the header opened' 'lib/formats::regimen::tests'
  seeded_case "two tables of one name accepted"       test     inject_regimen_table_collision_unchecked \
    'a table opened twice was not refused' 'lib/formats::regimen::tests'
  seeded_case "the float rule widened past the record" test    inject_regimen_float_rule_widened \
    'the regimen grammar and the record.s decimal disagree' 'lib/formats::regimen::tests'
  seeded_case "a summary the rows do not carry"       recompute inject_recompute_summary_not_derived \
    'the report does not re-derive'
  seeded_case "a results directory declaring no kind" recompute inject_recompute_kind_undeclared \
    'front-matter .kind. is None'
  seeded_case "a recompute that cannot fail"          recompute inject_recompute_cannot_fail \
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
    'holds its own binding and the table below it' 'lib/formats::regimen::tests'
  seeded_case "an array read by a second reader"      test     inject_regimen_array_second_reader \
    'an array item was read by something other than the value reader' 'lib/formats::regimen::tests'
  seeded_case "a merged field an injection cannot see" injections inject_injections_struct_grew \
    'builds a Provenance without cohort'
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
  seeded_case "a record missing its substrate"        results  inject_results_no_substrate \
    'says diet check-record: a .start. row is missing its required .substrate.'
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
  seeded_case "a dogma tag in no vocabulary"          test     inject_interview_tag_undeclared \
    'dogma tag\(s\) absent from diet/formats/interview/tags\.tsv' 'lib/formats::interview'
  seeded_case "operating points sorted, not in file order" test  inject_operating_points_sorted \
    "the projection lost the file's order" 'lib/formats::operating_points'
  seeded_case "an integer terminal grown a second time" test     inject_number_terminal_regrown \
    'it belongs in number\.pest and nowhere else' 'test:conformance/the_integer_terminal'
  seeded_case "historical, and carrying a recompute"   recompute inject_recompute_historical_with_a_script \
    'declares .historical-observation. and carries a recompute\.sh'
  seeded_case "historical with no reason stated"       recompute inject_recompute_historical_without_a_reason \
    'states no .historical_reason.'
  seeded_case "only the template recomputes"           recompute inject_recompute_only_the_template_recomputes \
    'results are present and none recomputed'
  seeded_case "a check no workflow runs"              ci       inject_ci \
    'has no owner in check-owners\.tsv'
  seeded_case "pull requests filtered by branch"      ci       inject_ci_pr_branch_filter \
    'carries .branches: \[main\]. and is reached'
  seeded_case "CI narrowing the test check"           ci       inject_ci_scoped_test \
    'passes .--scope. to verify\.sh'
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
  seeded_case "a forbidden id in a commit message"    history  inject_history \
    'hygiene: internal-ticket-id:'
  seeded_case "history with an undeterminable base"   history  inject_history_no_base \
    'an undeterminable base is a failure, not an empty scan'
  seeded_case "an ask wired to another class's question" test inject_router_ask_class_untuned \
    'the ask does not ask its own question' 'lib/capture::router::tests'
  seeded_case "a template without the imperative"     test     inject_router_ask_imperative_dropped \
    'the ask does not carry the fork-local imperative' 'lib/capture::router::tests'
  seeded_case "a census that miscounts which classes fired" test inject_router_census_class_miscounted \
    'the census does not say which classes fired' 'lib/capture::router::tests'
  seeded_case "a route verb answering with a hollow census" test inject_router_route_census_hollow \
    'where the drive spends' 'test:cli'
  seeded_case "a table row that can never fire"       test     inject_router_table_row_shadowed \
    'can never fire' 'lib/capture::router::tests'
  seeded_case "a corpus that stops covering a class"  test     inject_router_corpus_class_uncovered \
    'call\(s\) in the corpus, fewer than' 'lib/capture::router::tests'
  seeded_case "a table row skipped, not refused"      test     inject_router_table_row_skipped \
    'was skipped rather than refused' 'lib/capture::router::tests'
  seeded_case "an intent taken from any lane"         test     inject_router_intent_lane_ignored \
    'did not quote back what the model said it was about to do' 'lib/capture::router::tests'
  seeded_case "the first stated intent, not the last" test     inject_router_intent_first_not_last \
    'quoted an intent the model had already moved past' 'lib/capture::router::tests'
  seeded_case "an intent marker lost from the table"  test     inject_router_intent_marker_lost \
    'a marker was added or lost without a sentence that reaches it' 'lib/capture::router::tests'
  seeded_case "an unclassified call nobody can look up" test   inject_router_unclassified_unattributed \
    'must name the call, its turn, its tool and its word' 'lib/capture::router::tests'
  seeded_case "the declared default out of the vocabulary" test inject_router_class_vocabulary_shortened \
    'left the vocabulary without leaving the tests that walk it' 'lib/capture::router::tests'
  seeded_case "a quoted substitution descended into"  test     inject_mechanical_quoted_substitution \
    'a single-quoted substitution was descended into' 'lib'
  seeded_case "the declared default replaced by silence" test   inject_router_unknown_silent \
    'an unknown pattern must route to the declared default, never to silence' 'lib/capture::router::tests'
  seeded_case "a judgment ask released mid-turn"      test     inject_router_judgment_mid_turn \
    'a judgment ask fired in the middle of a turn' 'lib/capture::router::tests'
  seeded_case "a row of the routing table lost"       test     inject_router_table_row_lost \
    'misrouted call' 'lib/capture::router::tests'
  seeded_case "an unknown call routed but not recorded" test   inject_router_unclassified_silent \
    'an unknown pattern must be a typed event' 'lib/capture::router::tests'
  seeded_case "a reduction claimed, not computed"     test     inject_router_reduction_claimed \
    'the reduction is not the number its own counts give' 'lib/capture::router::tests'
  seeded_case "a subshell that shares the parent state" test   inject_mechanical_subshell_leaks \
    'the subshell cd leaked into the parent' 'lib/capture::mechanical::tests'
  seeded_case "a failed cd applied anyway"            test     inject_mechanical_failed_cd_applied \
    'a failed cd moved the working directory' 'lib/capture::mechanical::tests'
  seeded_case "popd on an empty stack ignored"        test     inject_mechanical_popd_empty_ignored \
    'popd on an empty stack was silently ignored' 'lib/capture::mechanical::tests'
  seeded_case "the mechanical-noun table emptied"     test     inject_mechanical_lint_table_emptied \
    'a question about a mechanical fact went unflagged' 'lib/capture::mechanical::tests'
  seeded_case "a mechanical entry sent through the gate" test  inject_mechanical_entry_grounded \
    'a mechanical entry was dropped as if it needed grounding' 'lib/capture::mechanical::tests'
  seeded_case "a cosine that forgot its second norm"  test     inject_sense_cosine_unnormalised \
    'cosine of a vector with itself was not one' 'lib/capture'
  seeded_case "contrastive scoring that ignores the negative sense" test inject_sense_contrastive_ignores_negative \
    'the contrastive score ignored the negative sense' 'lib/capture::sense::tests'
  seeded_case "a null whose labels are never shuffled" test   inject_sense_null_labels_unshuffled \
    'd-prime on a shuffled-label null was far from zero' 'lib/capture::sense::tests'
  seeded_case "a bootstrap p with no attainable floor" test   inject_sense_p_without_floor \
    'a bootstrap p-value came without its attainable floor' 'lib/capture::sense::tests'
  seeded_case "a metric whose failure fixture is gone" test   inject_sense_metric_fixture_removed \
    'no failure fixture, so it can never be reported' 'lib/capture::sense::tests'
  seeded_case "a register mislabelled at its source" test     inject_sense_register_source_mislabelled \
    'and a row says otherwise' 'lib/capture::sense::tests'
  seeded_case "a file in the register naming nothing"  test     inject_sense_register_unnamed_file \
    'not a declared sidecar' 'lib/capture::sense::tests'
  seeded_case "a mined row nobody can trace"          test     inject_sense_provenance_join_dropped \
    'a row nobody can trace was accepted' 'lib/capture::sense::tests'
  seeded_case "two data lines read as one"            test     inject_record_data_line_two_lines \
    'two lines were read as one, and the second was lost' 'lib/formats::record::json::tests'
  seeded_case "controls that never look at the register" test inject_sense_controls_ignore_register \
    'a register row reached the top control and the controls passed' 'lib/capture::sense::tests'
  seeded_case "a bootstrap that never resamples"      test     inject_sense_bootstrap_never_resamples \
    'every resample was the observed difference' 'lib/capture::sense::tests'
  seeded_case "a failure reading off the worst reading" test   inject_sense_failure_reading_moved \
    'is the worst the metric can say' 'lib/capture::sense::tests'
  seeded_case "a pre-registration with nothing in it" test     inject_sense_pre_registration_emptied \
    'the primary endpoint is not the endpoint that was registered' 'lib/capture::sense::tests'
  seeded_case "a lexical gate asked about the id"     test     inject_sense_gate_reads_the_id \
    'the gate did not decide on the row' 'lib/capture::sense::tests'
  seeded_case "a separation over one class spread"    test     inject_sense_d_prime_unpooled \
    'd-prime was standardised by one class' 'lib/capture::sense::tests'
  seeded_case "a null band widened past a finding"    test     inject_sense_null_band_widened \
    'bands are not the numbers they were registered as' 'lib/capture::sense::tests'
  seeded_case "a reported metric that reports a constant" test inject_sense_reported_value_constant \
    'the record of a metric is not the numbers the metric produced' 'lib/capture::sense::tests'
  seeded_case "the mechanical lane renamed"           test     inject_mechanical_lane_renamed \
    'the lane was renamed' 'lib/capture::mechanical::tests'
  seeded_case "an option word read as a directory"    test     inject_mechanical_option_is_a_directory \
    'an option word was read as the directory it names' 'lib/capture::mechanical::tests'
  seeded_case "a flag value read as a file"           test     inject_mechanical_flag_value_is_a_file \
    'the lane read a file out of a flag.s value' 'lib/capture::mechanical::tests'
  seeded_case "an entry with the wrong verb"          test     inject_mechanical_entry_verb_swapped \
    'the entry used the wrong verb for what happened to the file' 'lib/capture::mechanical::tests'
  seeded_case "a pipeline whose members never run"    test     inject_mechanical_pipeline_skipped \
    'nothing in the pipeline ran' 'lib/capture::mechanical::tests'
  seeded_case "a write lost to a read of the same path" test   inject_mechanical_write_lost_to_a_read \
    'a write was lost to a read of the same path in the same call' 'lib/capture::mechanical::tests'
  seeded_case "the resolver keying on a comment"       resolver inject_keyed_by_a_comment \
    'keyed as .*inject_alpha'
  seeded_case "a run directory inside a run directory" results inject_results_nested_directory \
    'a run directory inside a run directory'
  seeded_case "a verdict read by prefix"               test     inject_verdict_prefix_accepted \
    'verdict-as-a-prefix\.txt: accepted as' 'test:conformance/formats::verdict'
  seeded_case "an identifier in a reason read as a verdict" test inject_verdict_identifier_read_as_a_verdict \
    'reason-naming-an-identifier\.txt: rejected' 'test:conformance/formats::verdict'
  seeded_case "a second verdict in a reason accepted"  test     inject_verdict_second_verdict_accepted \
    'two-verdicts\.txt: accepted as' 'test:conformance/formats::verdict'
  seeded_case "an anchor matched inside a longer word"  test     inject_collector_substring_match \
    'an anchor matched inside a longer word' 'lib/capture::collector::literal::tests'
  seeded_case "an English word made an anchor"         test     inject_collector_english_anchor \
    'an English word became an anchor' 'lib/capture::collector::literal::tests'
  seeded_case "an entry nominated by its own turn"     test     inject_collector_self_nomination \
    'an entry nominated itself' 'lib/capture::collector::literal::tests'
  seeded_case "a supersession that adds without voiding" test   inject_reconcile_supersede_without_voiding \
    'the old entry was not voided' 'lib/capture::collector::reconcile::tests'
  seeded_case "a verdict that settles nothing settling"  test   inject_reconcile_partial_applies_a_patch \
    'PARTIAL produced a patch' 'lib/capture::collector::reconcile::tests'
  seeded_case "an uncalibrated nomination policy accepted" test inject_collector_uncalibrated_policy \
    'the fixture policy was accepted by the door that ships' 'lib/capture::collector::sense::tests'
  seeded_case "a nomination budget ignored"           test     inject_collector_budget_ignored \
    'the budget did not bind' 'lib/capture::collector::sense::tests'
  seeded_case "a turn that nominates only its first entry" test inject_collector_one_nomination_per_turn \
    'a turn that named two anchors nominated fewer than two entries' 'lib/capture::collector::literal::tests'
  seeded_case "a voided entry nominated by tier 0" test inject_collector_voided_entry_renominated \
    'a voided entry was nominated again by the literal tier' 'lib/capture::collector::literal::tests'
  seeded_case "a hit that says nothing about where" test inject_collector_hit_offset_lost \
    'the hits did not say where the anchor recurred' 'lib/capture::collector::literal::tests'
  seeded_case "an overlapping anchor scan" test inject_collector_overlapping_scan \
    'an anchor that overlaps itself was counted at every shifted position' 'lib/capture::collector::literal::tests'
  seeded_case "a double-quoted span that is not an anchor" test inject_collector_quoted_anchor_delimiter \
    'a double-quoted span was not anchored' 'lib/capture::collector::literal::tests'
  seeded_case "a two-byte shape read as an anchor" test inject_collector_short_shape_anchored \
    'a two-byte shape was anchored' 'lib/capture::collector::literal::tests'
  seeded_case "a module path that is not an identifier" test inject_collector_module_path_shape \
    'a path through the module tree was not anchored' 'lib/capture::collector::literal::tests'
  seeded_case "a sentence word read as a file extension" test inject_collector_extension_window \
    'a dotted word whose tail is a word was read as a file name' 'lib/capture::collector::literal::tests'
  seeded_case "an anchor with the sentence still on it" test inject_collector_token_untrimmed \
    'the punctuation prose hung on a token was kept as part of the anchor' 'lib/capture::collector::literal::tests'
  seeded_case "anchor kinds that swapped their names" test inject_collector_anchor_kind_permuted \
    'the anchor kind vocabulary is not what it promises' 'lib/capture::collector::tests'
  seeded_case "a source vocabulary with no members" test inject_collector_source_vocabulary_emptied \
    'the source vocabulary is not what it promises' 'lib/capture::collector::tests'
  seeded_case "a nomination that names the other tier" test inject_collector_tier_name_swapped \
    'a nomination named the wrong tier' 'lib/capture::collector::tests'
  seeded_case "registers that swapped their names" test inject_collector_register_permuted \
    'the register vocabulary is not what it promises' 'lib/capture::collector::tests'
  seeded_case "a lexical pre-gate the tier ignores" test inject_collector_gate_ignored \
    'a turn carrying no seed was scored by a gated policy anyway' 'lib/capture::collector::sense::tests'
  seeded_case "a turn-level threshold never compared" test inject_collector_turn_threshold_ignored \
    'an unremarkable turn nominated' 'lib/capture::collector::sense::tests'
  seeded_case "a per-entry threshold never compared" test inject_collector_entry_threshold_ignored \
    'the tier nominated an entry the turn was not about' 'lib/capture::collector::sense::tests'
  seeded_case "a turn scored against the wrong sense set" test inject_collector_wrong_sense_set \
    'a turn about a mistaken assumption was scored as a reversal' 'lib/capture::collector::sense::tests'
  seeded_case "an intent register that reads the prose" test inject_collector_intent_register_reads_the_prose \
    'the tier measured the prose of the turn where it should have measured the stated intent' 'lib/capture::collector::sense::tests'
  seeded_case "a budget spent on the worst candidates" test inject_collector_budget_takes_the_worst \
    'the budget was spent on the entries that scored lowest' 'lib/capture::collector::sense::tests'
  seeded_case "a nomination score nothing measured" test inject_collector_score_not_measured \
    'a nomination carried a score nothing measured' 'lib/capture::collector::sense::tests'
  seeded_case "an entry nominated by its own turn at tier 1" test inject_collector_sense_self_nomination \
    'an entry born in this turn nominated itself' 'lib/capture::collector::sense::tests'
  seeded_case "a voided entry nominated by tier 1" test inject_collector_sense_voided_entry_renominated \
    'a voided entry was nominated again by the sense tier' 'lib/capture::collector::sense::tests'
  seeded_case "a report that drops half its policy" test inject_collector_report_drops_the_policy \
    'the report did not say which scoring produced it' 'lib/capture::collector::sense::tests'
  seeded_case "a calibration that names no run" test inject_collector_policy_names_no_run \
    'was read as a calibration' 'lib/capture::collector::sense::tests'
  seeded_case "a supersession that writes a constant" test inject_reconcile_supersede_writes_a_constant \
    'the superseding entry does not say what superseded the old one' 'lib/capture::collector::reconcile::tests'
  seeded_case "a supersession with no fork behind it" test inject_reconcile_supersede_loses_its_fork \
    'the superseding entry does not say which fork produced it' 'lib/capture::collector::reconcile::tests'
  seeded_case "a patch the reconciler never hands back" test inject_reconcile_patch_never_handed_back \
    'a verdict that produced a patch did not hand it back' 'lib/capture::collector::reconcile::tests'
  seeded_case "a PARTIAL counted as a false nomination" test inject_reconcile_partial_read_as_a_false_nomination \
    'a fork that said the prose bears on the entry was read as a false nomination' 'lib/capture::collector::reconcile::tests'
  seeded_case "a mention applied as a supersession" test inject_reconcile_mention_superseded \
    'NOT_THIS produced a patch' 'lib/capture::collector::reconcile::tests'
  seeded_case "self-capture exempt from grounding"    test     inject_tools_ungrounded \
    'modality does not exempt a lane from grounding' 'lib/capture::tools::tests'
  seeded_case "a reminder cadence that never fires"   test     inject_tools_reminder_silent \
    'ten silent turns must be reminded at every third turn' 'lib/capture::tools::tests'
  seeded_case "a harness tool call read as a capture" test     inject_tools_foreign_call \
    'a harness tool call is not a capture tool and writes nothing' 'lib/capture::tools::tests'
  seeded_case "a phase proposal that writes a fact"   test     inject_tools_proposal_writes \
    'a phase-transition proposal is advisory and writes nothing' 'lib/capture::tools::tests'
  seeded_case "a capture grounded in its own echo"   test     inject_tools_self_echo \
    'a harness that repeats a capture back grounded the capture in itself' 'lib/capture::tools::tests'
  seeded_case "a capture grounded in a later turn"   test     inject_tools_future_output \
    'a turn-1 capture was grounded in a turn-2 tool output' 'lib/capture::tools::tests'
  seeded_case "a superseding entry with a minted id" test     inject_tools_supersede_minted \
    'a superseding entry id must be derived from the row that carried it' 'lib/capture::tools::tests'
  seeded_case "a verdict that resolves elsewhere"    test     inject_tools_resolve_elsewhere \
    'a verdict resolved an entry the model never named' 'lib/capture::tools::tests'
  seeded_case "the asks reworded to nothing"         test     inject_tools_ask_words \
    'the words this lane says out loud are the only product it has' 'lib/capture::tools::tests'
  seeded_case "an ask that drops its own question"   test     inject_tools_ask_question_dropped \
    'an ask carrying a deferral is the question and then the deferral' 'lib/capture::tools::tests'
  seeded_case "the sweep asking as the cadence"      test     inject_tools_sweep_kind \
    'the sweep and the cadence are one ask wearing two names' 'lib/capture::tools::tests'
  seeded_case "a closed choice left in its own case" test     inject_tools_choice_uncanonical \
    'a harness may shout a closed choice back at us' 'lib/capture::tools::tests'
  seeded_case "a phase proposal with no reason"      test     inject_tools_proposal_reasonless \
    'the reason is the whole of what it carries into one' 'lib/capture::tools::tests'
  seeded_case "a reminder that drops the deferral"   test     inject_tools_reminder_deferral_dropped \
    'the cadence reminded without what the router put off in that turn' 'lib/capture::tools::tests'
  seeded_case "tools described in one character"     test     inject_tools_description_thin \
    'a foreign harness registers this text verbatim' 'lib/capture::tools::tests'
  seeded_case "an offered tool the contract omits"   test     inject_tools_contract_undescribed \
    'an offered tool with no row is refused before a harness ever sees it' 'lib/capture::tools::tests'
  seeded_case "one tool described twice"             test     inject_tools_contract_duplicate \
    'one tool described twice leaves the harness to the order of the file' 'lib/capture::tools::tests'
  seeded_case "a verdict the lane cannot honour"     test     inject_tools_verdict_list_open \
    'this lane refuses it at runtime: the model is invited to say a word' 'lib/capture::tools::tests'
  seeded_case "a turn swept after it recorded"       test     inject_tools_silent_kept \
    'a turn the model did record in the end was swept anyway' 'lib/capture::tools::tests'
  seeded_case "the object reader with no limit"      test     inject_record_objects_undepthed \
    'one level past the limit must be a verdict from `objects`' 'lib/formats::record::tests'
  seeded_case "a corpus that drives one tool"       test     inject_tools_corpus_one_tool \
    'is offered to the model and no corpus case ever calls it' 'lib/capture::tools::tests'
  seeded_case "the ablation's control arm dropped"     test     inject_ablation_no_control \
    'the control arm is missing: an ablation with no sentence-removed arm' 'lib/capture::ablation::tests'
  seeded_case "silence counted as engagement"          test     inject_ablation_silence_engages \
    'counted as engagement and as silence at once' 'lib/capture::ablation::tests'
  seeded_case "a p reported without its floor"         test     inject_ablation_p_floor_dropped \
    'a p reported without its attainable floor' 'lib/capture::ablation::tests'
  seeded_case "a resample that draws once"            test     inject_ablation_resample_single_draw \
    'the resamples no longer draw one outcome for every fork' 'lib/capture::ablation::tests'
  seeded_case "a generator that never advances"       test     inject_ablation_frozen_generator \
    'a seeded draw did not reach every fork in 64 tries' 'lib/capture::ablation::tests'
  seeded_case "each arm credited with the other's"    test     inject_ablation_rates_swapped \
    'the first arm held on 5 of these 6 forks and the bootstrap counted' 'lib/capture::ablation::tests'
  seeded_case "the silence endpoint unregistered"     test     inject_ablation_endpoint_dropped \
    'the pre-registration no longer carries two endpoints' 'lib/capture::ablation::tests'
  seeded_case "one imperative for every arm"          test     inject_ablation_plan_one_imperative \
    'the plan does not pair the arm' 'lib/capture::ablation::tests'
  seeded_case "two arms under one name"               test     inject_ablation_arm_names_collide \
    'two arms of the ablation are reported under one name' 'lib/capture::ablation::tests'
  seeded_case "an arm's clauses run together"         test     inject_ablation_clauses_run_together \
    'the imperative an arm puts in the fork is not its clauses separated by one' 'lib/capture::ablation::tests'
  seeded_case "a placeholder nobody counts"           test     inject_ablation_placeholder_word_dropped \
    'the-placeholder-words-nobody-replaced: graded .engaged. where the corpus says .inert.' 'lib/capture::ablation::tests'
  seeded_case "a blank clause text admitted"          test     inject_ablation_blank_clause_allowed \
    'a clause table with a blank clause text was not refused for that reason' 'lib/capture::ablation::tests'
  seeded_case "a grading case quietly dropped"        test     inject_ablation_corpus_case_dropped \
    'the corpus holds a case with no expectation or an expectation with no case' 'lib/capture::ablation::tests'
  seeded_case "an untagged decline read as content"   test     inject_ablation_untagged_decline_engages \
    'an-untagged-decline: graded .engaged. where the corpus says .inert.' 'lib/capture::ablation::tests'
  seeded_case "the second reader left unbounded"      test     inject_json_objects_unbounded \
    'a JSON Lines reader that does not bound its nesting' 'lib/formats::record::json::tests'

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
  for dir in "${ROOT}"/tests/fixtures/results-bad/*/; do
    in_shard || continue
    rc=0; name="$(basename "$dir")"
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
  done

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
  python3 - "${relay}/2026-01-30-no-substrate/run.jsonl" <<'EOF'
import json, pathlib, sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text(encoding="utf-8").split("\n")
row = json.loads(lines[0])
del row["regime"]["substrate"]
lines[0] = json.dumps(row, separators=(",", ":"))
path.write_text("\n".join(lines), encoding="utf-8")
EOF
  expect_exit "a record diet refuses gets no verdict from the linter" 0 \
    bash -c "cd '${ROOT}' && cargo build --quiet -p discipline-diet --bin diet \
      && out=\$(python3 scripts/check-results.py '${relay}/2026-01-30-no-substrate' 2>&1; true) \
      && grep -q 'says diet check-record: a .start. row is missing its required .substrate.' <<<\"\$out\" \
      && ! grep -qE 'front-matter|summary row|product_sha256' <<<\"\$out\" \
      && grep -q 'record verdicts from .* sha256=' <<<\"\$out\""

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
  expect_exit "a pin spelled with ./ is the same pin" 0 \
    bash -c "cd '${pin}' && CARGO_TARGET_DIR= DIET_BIN=./diet \
      python3 '${ROOT}/scripts/resolve-diet.py' --expect ./diet"

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


def write(case, shard, ordinals, said_total=None):
    # Each shard's artifact arrives in a directory of its own, as
    # download-artifact leaves them.
    where = root / case / f"shard-{shard}"
    where.mkdir(parents=True, exist_ok=True)
    (where / "census.tsv").write_text(
        f"shard\t{shard}\nshards\t{SHARDS}\ntotal\t{total if said_total is None else said_total}\n"
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
  if [ -n "$SELFTEST_CENSUS" ]; then
    {
      printf 'shard\t%d\n' "$(( SELFTEST_SHARD == 0 ? 1 : SELFTEST_SHARD ))"
      printf 'shards\t%d\n' "$SELFTEST_SHARDS"
      printf 'total\t%d\n' "$SELFTEST_UNITS"
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

if [ "$mode" = "selftest" ]; then
  rc=0
  selftest || rc=$?
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
  exit "$EXIT_FAIL"
fi
printf 'verify: %d check(s) passed.\n' "${#selected[@]}"
