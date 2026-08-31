#!/usr/bin/env bash
#
# The gate. Everything that must be true of this repository is checked here,
# and CI runs exactly this script.
#
#   verify.sh                 run every check
#   verify.sh --only CHECK    run one check (repeatable)
#   verify.sh --list          name the checks, in order
#   verify.sh --selftest      prove the gate goes red on seeded faults
#
# Two rules this script exists to keep:
#
#   * Bind to real exit codes. No `cmd | tee`, no `cmd | grep`, nothing that
#     puts a pipeline's status where a command's status belongs. Every check
#     runs with its own streams and its status is captured directly.
#   * A gate that has never been seen red is not a gate. `--selftest` copies
#     the tree, injects one deliberate fault per check, and runs *this same
#     script* against it, reporting the exit code it observed.
#
# Exit 0 if every check passed, 1 if any failed, 2 if the script was misused.

set -euo pipefail

readonly EXIT_FAIL=1
readonly EXIT_MISUSE=2

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT

readonly CHECKS=(fmt clippy test results regimen hygiene)

FAILED=()

# --------------------------------------------------------------------------
# checks
# --------------------------------------------------------------------------

check_fmt() { cargo fmt --all --check; }

check_clippy() { cargo clippy --workspace --all-targets -- -D warnings; }

check_test() { cargo test --workspace; }

check_results() { python3 scripts/check-results.py --root results; }

# Every regimen.toml under results/ must parse as a `regimen` document. This
# is what keeps diet/formats load-bearing: the format is used on the
# repository's own data, not merely defined.
check_regimen() {
  local target="${CARGO_TARGET_DIR:-target}"
  cargo build --quiet -p exercise
  local bin="${target}/debug/exercise"
  [ -x "$bin" ] || { echo "verify: no exercise binary at ${bin}" >&2; return 2; }

  local rc=0 seen=0 file
  while IFS= read -r -d '' file; do
    seen=$((seen + 1))
    if "$bin" "$file" > /dev/null; then
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

check_hygiene() { bash scripts/hygiene.sh; }

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

# Copy everything git tracks or would track into DEST, and make it a git
# repository so that checks which ask git what exists still work.
sandbox() {
  local dest="$1" path
  mkdir -p "$dest"
  while IFS= read -r -d '' path; do
    mkdir -p "${dest}/$(dirname "$path")"
    cp -p "$path" "${dest}/${path}"
  done < <(git -C "$ROOT" ls-files -z --cached --others --exclude-standard)
  git -C "$dest" init --quiet
  git -C "$dest" add --all
}

# Run `verify.sh --only CHECK` inside a sandbox carrying one seeded fault, and
# report the exit code observed. Red is the expected outcome.
seeded_case() {
  local label="$1" check="$2" inject="$3"
  local box
  box="$(mktemp -d)"
  sandbox "$box"
  ( cd "$box" && "$inject" )

  local rc=0
  ( cd "$box" && CARGO_TARGET_DIR="${SELFTEST_TARGET}" bash ./verify.sh --only "$check" ) \
    > "${box}.log" 2>&1 || rc=$?

  if [ "$rc" -ne 0 ]; then
    printf 'RED   verify.sh --only %-8s exit %-3d  %s\n' "$check" "$rc" "$label"
  else
    printf 'GREEN verify.sh --only %-8s exit %-3d  %s  <-- THE GATE DID NOT FIRE\n' \
      "$check" "$rc" "$label"
    SELFTEST_BROKEN+=("${label}")
    sed -n '1,40p' "${box}.log" >&2
  fi
  rm -rf "$box" "${box}.log"
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

inject_conformance() {
  printf 'budget = 1.5\n' > diet/formats/regimen/fixtures/valid/seeded-nonconforming.toml
  printf '{ "budget": { "integer": 1 } }\n' > diet/formats/regimen/fixtures/valid/seeded-nonconforming.expected.json
}

inject_results() {
  cp -r tests/fixtures/results-bad/2026-01-09-unbacked-number results/
}

inject_regimen() {
  printf 'arm = 1.5\n' > results/_template/regimen.toml
}

inject_hygiene() {
  bash scripts/seed-hygiene-fault.sh seeded-faults > /dev/null
  git add --all
}

selftest() {
  SELFTEST_TARGET="$(mktemp -d)/target"
  SELFTEST_BROKEN=()
  export SELFTEST_TARGET

  echo "Seeded-fault selftest. Every line below must read RED: the fault is"
  echo "deliberate and the gate is what is under test."
  echo

  seeded_case "misformatted source"                  fmt      inject_fmt
  seeded_case "clippy lint violation"                clippy   inject_clippy
  seeded_case "failing unit test"                    test     inject_test
  seeded_case "non-conforming format fixture"        test     inject_conformance
  seeded_case "results claim unbacked by run.jsonl"  results  inject_results
  seeded_case "regimen.toml that is not a regimen"   regimen  inject_regimen
  seeded_case "forbidden content in the tree"        hygiene  inject_hygiene

  echo
  echo "--- results fixtures, checked directly ---"
  local dir rc
  for dir in "${ROOT}"/tests/fixtures/results-bad/*/; do
    rc=0
    python3 "${ROOT}/scripts/check-results.py" "$dir" > /dev/null 2>&1 || rc=$?
    if [ "$rc" -eq 1 ]; then
      printf 'RED   check-results.py exit %-3d  %s\n' "$rc" "$(basename "$dir")"
    else
      printf 'GREEN check-results.py exit %-3d  %s  <-- FIXTURE DID NOT FAIL\n' \
        "$rc" "$(basename "$dir")"
      SELFTEST_BROKEN+=("results fixture $(basename "$dir")")
    fi
  done

  echo
  echo "--- hygiene patterns, each proven against its own class ---"
  local seed label out
  seed="$(mktemp -d)"
  bash "${ROOT}/scripts/seed-hygiene-fault.sh" "$seed" > /dev/null
  for dir in "$seed"/*/; do
    label="$(basename "$dir")"
    rc=0
    out="$(bash "${ROOT}/scripts/hygiene.sh" --tree "$dir" 2>&1)" || rc=$?
    if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q "hygiene: ${label}:"; then
      printf 'RED   hygiene.sh exit %-3d  %s\n' "$rc" "$label"
    else
      printf 'GREEN hygiene.sh exit %-3d  %s  <-- PATTERN DID NOT FIRE\n' "$rc" "$label"
      SELFTEST_BROKEN+=("hygiene pattern ${label}")
    fi
  done
  rm -rf "$seed"

  echo
  if [ "${#SELFTEST_BROKEN[@]}" -gt 0 ]; then
    printf 'selftest: %d gate(s) failed to fire:\n' "${#SELFTEST_BROKEN[@]}"
    printf '  - %s\n' "${SELFTEST_BROKEN[@]}"
    return "$EXIT_FAIL"
  fi
  echo "selftest: every gate was seen red on its seeded fault."
}

# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

selected=()
mode="all"

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
    --list) printf '%s\n' "${CHECKS[@]}"; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "verify: unknown argument '$1'" >&2; usage >&2; exit "$EXIT_MISUSE" ;;
  esac
done

cd "$ROOT"

if [ "$mode" = "selftest" ]; then
  selftest
  exit $?
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
