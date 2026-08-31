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

readonly CHECKS=(fmt clippy test results regimen metadata hygiene pages)

# The forbidden classes the genesis brief names by hand. Pinning them here
# means a pattern row cannot be deleted along with its seeded class and leave
# the selftest still reporting success.
readonly REQUIRED_HYGIENE_CLASSES=(
  private-ipv4 internal-hostname personal-home-path windows-user-path
  internal-ticket-id private-key-block
)
readonly REQUIRED_PAGES_CLASSES=(
  external-subresource external-stylesheet network-call form-element
)

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
#
# `cargo run` rather than a hand-built path, so that the binary under test is
# always the one cargo would produce for this tree -- honouring
# build.target-dir and any other configuration -- and never a stale artifact
# left in a shared target directory.
check_regimen() {
  local rc=0

  # The grammar declares regimen to be a subset of TOML, and that claim is
  # load-bearing: the same regimen.toml is read by this grammar and by
  # tomllib. Check the claim rather than trusting it.
  python3 scripts/check-toml-subset.py || rc=$?

  cargo build --quiet -p exercise || {
    local build_rc=$?
    echo "verify: exercise did not build (exit ${build_rc})" >&2
    return "$build_rc"
  }

  local seen=0 file
  while IFS= read -r -d '' file; do
    seen=$((seen + 1))
    if cargo run --quiet -p exercise -- "$file" > /dev/null; then
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

check_metadata() { python3 scripts/check-repo-metadata.py; }

check_hygiene() { bash scripts/hygiene.sh; }

# The site published to gh-pages is static, and this is what makes that a gate
# rather than a promise: no subresource from another origin, no network call,
# no form, no credential shapes.
check_pages() {
  bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree pages
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
# selftest
# --------------------------------------------------------------------------

SELFTEST_SCRATCH=()
SELFTEST_BROKEN=()
SEEDED_CHECKS=()

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
  SCRATCH="$(mktemp -d)"
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
  mkdir -p "$dest"
  while IFS= read -r -d '' path; do
    if [ ! -e "${ROOT}/${path}" ]; then
      echo "selftest: git lists ${path} but it is not on disk" >&2
      return 1
    fi
    mkdir -p "${dest}/$(dirname -- "$path")"
    cp -L -- "${ROOT}/${path}" "${dest}/${path}"
  done < <(git -C "$ROOT" ls-files -z --cached --others --exclude-standard)
  git -C "$dest" init --quiet
  git -C "$dest" add --all
}

# Run `verify.sh --only CHECK` inside a sandbox carrying one seeded fault.
#
# EXPECT is an extended regex the sandbox's log must carry. The exit code is
# the verdict -- the signature decides whether that verdict is about the fault
# we seeded rather than something incidental.
seeded_case() {
  local label="$1" check="$2" inject="$3" expect="$4"
  local box
  scratch; box="$SCRATCH"
  SEEDED_CHECKS+=("$check")

  sandbox "$box"
  ( cd "$box" && "$inject" )

  local rc=0
  ( cd "$box" && CARGO_TARGET_DIR="${SELFTEST_TARGET}" bash ./verify.sh --only "$check" ) \
    > "${box}.log" 2>&1 || rc=$?

  if [ "$rc" -eq 0 ]; then
    printf 'GREEN verify.sh --only %-8s exit %-3d  %s  <-- THE GATE DID NOT FIRE\n' \
      "$check" "$rc" "$label"
    SELFTEST_BROKEN+=("${label}: the gate did not fire")
    sed -n '1,40p' "${box}.log" >&2
  elif ! grep -qE -- "$expect" "${box}.log"; then
    printf 'WRONG verify.sh --only %-8s exit %-3d  %s  <-- RED, BUT NOT FOR ITS OWN FAULT\n' \
      "$check" "$rc" "$label"
    printf '      the log carries no match for: %s\n' "$expect"
    SELFTEST_BROKEN+=("${label}: red for the wrong reason")
    sed -n '1,40p' "${box}.log" >&2
  else
    printf 'RED   verify.sh --only %-8s exit %-3d  %s\n' "$check" "$rc" "$label"
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
  sed -i 's/&\["regimen"\]/\&[]/' diet/src/formats/mod.rs
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
    defined+=("$label")

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

selftest() {
  trap selftest_cleanup EXIT
  scratch; SELFTEST_TARGET="${SCRATCH}/target"

  echo "Seeded-fault selftest. Every line below must read RED: the fault is"
  echo "deliberate and the gate is what is under test. A case that goes red"
  echo "without its own signature in the log reads WRONG, not RED."
  echo

  seeded_case "misformatted source"                   fmt      inject_fmt \
    'Diff in .*diet/src/lib\.rs'
  seeded_case "clippy lint violation"                 clippy   inject_clippy \
    'ptr_arg'
  seeded_case "failing unit test"                     test     inject_test \
    'seeded_fault::seeded_failure'
  seeded_case "non-conforming format fixture"         test     inject_conformance \
    'valid_fixtures_parse_to_their_expected_value'
  seeded_case "FORMATS emptied, harness covers none"  test     inject_formats_empty \
    'FORMATS is empty'
  seeded_case "results claim contradicts run.jsonl"   results  inject_results \
    'front-matter `turns` states 3 but the summary record binds'
  seeded_case "regimen.toml that is not a regimen"    regimen  inject_regimen \
    'BAD results/_template/regimen\.toml'
  seeded_case "a valid regimen that is not TOML"      regimen  inject_toml_subset \
    'are not TOML'
  seeded_case "template label nothing defines"        metadata inject_metadata \
    'assigns label'
  seeded_case "forbidden content in the tree"         hygiene  inject_hygiene \
    'hygiene: internal-ticket-id:'
  seeded_case "external subresource on the site"      pages    inject_pages \
    'hygiene: external-subresource:'

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
