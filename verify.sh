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

readonly CHECKS=(fmt clippy test library results recompute regimen metadata hygiene pages ci history injections parity)

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

# `--no-fail-fast` because cargo otherwise stops at the first test binary that
# fails, and the failures it then never prints are the ones a seeded case has
# to recognise: a broken unit test in the library would hide every conformance
# failure behind it, and the log would say the gate fired for the wrong reason.
check_test() { cargo test --workspace --no-fail-fast; }

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
  mkdir -p "$dest"
  while IFS= read -r -d '' path; do
    # -f after dereference: a broken symlink, or one pointing at a directory,
    # would make `cp` fail mid-copy and leave a half-built sandbox.
    if [ ! -f "${ROOT}/${path}" ]; then
      echo "selftest: ${path} is not a regular file (missing, or a symlink to one)" >&2
      return 1
    fi
    mkdir -p "${dest}/$(dirname -- "$path")"
    cp -L -- "${ROOT}/${path}" "${dest}/${path}"
  done < <(git -C "$ROOT" ls-files -z --cached --others --exclude-standard)
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
  local label="$1" check="$2" inject="$3" expect="$4"
  local box
  scratch; box="$SCRATCH"
  SEEDED_CHECKS+=("$check")

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
    printf 'BROKEN verify.sh --only %-8s          %s  <-- THE SANDBOX COULD NOT BE BUILT\n' \
      "$check" "$label"
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
      printf 'BROKEN verify.sh --only %-8s          %s  <-- THE SANDBOX COULD NOT BE READ\n' \
        "$check" "$label"
      SELFTEST_BROKEN+=("${label}: the sandbox's state could not be read")
      return
      ;;
  esac
  # Every sandbox shares SELFTEST_TARGET, and a test binary bakes its
  # CARGO_MANIFEST_DIR in at compile time. So a binary cargo judges fresh and
  # reuses reads the FIXTURES OF THE BOX IT WAS BUILT IN -- a stale-artifact
  # false receipt, one directory over from the one this selftest already
  # carries a comment about. It is safe today only because every injection
  # happens to edit Rust source and so forces a rebuild; one that touched only
  # a fixture would silently test the wrong tree. Touching a source file after
  # the fingerprint is taken removes the coincidence. `git write-tree` hashes
  # content, so this does not disturb the comparison above.
  #
  # IN THE BOX. The first version of this line was relative, and only the
  # injection above runs inside the box -- so it touched the repository's own
  # lib.rs on every case, forced no rebuild where it meant to, and left the
  # repository's binary older than its source for anything that resolved it
  # afterwards. The results-fixture loop below found that out.
  touch "${box}/diet/src/lib.rs" 2> /dev/null || true

  if [ "$state_before" = "$state_after" ]; then
    printf 'BROKEN verify.sh --only %-8s          %s  <-- THE INJECTION CHANGED NOTHING\n' \
      "$check" "$label"
    SELFTEST_BROKEN+=("${label}: ${inject} changed nothing, so the case proves nothing")
    return
  fi

  # Hermetic: only scripts/hermetic.sh's allowlist reaches the sandbox. A
  # blocklist would silently admit every variable nobody thought of, and a
  # sandbox that can see the ambient CI identity makes a check which reads it
  # behave differently there than a contributor would ever see.
  local rc=0
  ( cd "$box" && bash "${ROOT}/scripts/hermetic.sh" \
      env CARGO_TARGET_DIR="${SELFTEST_TARGET}" bash ./verify.sh --only "$check" ) \
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

# A table header that opens nothing. Every key lands at the top level, so
# `[sampler] seed` and a document-level `seed` become one binding and the arm
# that named both is recorded as an arm that named one.
inject_regimen_table_scope_flattened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "                let scope = match &table {\n"
new = "                let scope = match &None::<String> {\n"
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
old = "                    Some(Value::Table(_)) => return Err(ParseError::DuplicateTable { name }),\n"
new = "                    Some(Value::Table(_)) => {}\n"
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
old = '{"record":"summary","turns":2,'
new = '{"record":"summary","turns":3,'
assert old in source
record.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A directory that declares no kind. It is then neither recomputed nor counted
# as knowingly skipped, and the census that says so is the only thing standing
# between "nothing to check here" and "nothing was checked".
inject_recompute_kind_undeclared() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("results/_template/README.md")
source = path.read_text(encoding="utf-8")
old = 'kind = "reproducible-by-config"\n'
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
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

inject_ci() {
  # Take a check's owner away: it then runs in no workflow, while CI is green.
  sed -i '/^hygiene\t/d' .github/check-owners.tsv
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
}

selftest() {
  trap selftest_cleanup EXIT
  scratch; SELFTEST_TARGET="${SCRATCH}/target"

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
    'seeded_fault::seeded_failure \.\.\. FAILED'
  seeded_case "non-conforming format fixture"         test     inject_conformance \
    'conformance failure\(s\)'
  seeded_case "FORMATS emptied, harness covers none"  test     inject_formats_empty \
    'FORMATS is empty'
  seeded_case "decline grammar loses its end anchor"  test     inject_decline_unanchored \
    'coordinated-with-and\.txt: accepted as'
  seeded_case "interview drops continuation lines"    test     inject_interview_drops_continuations \
    'multi-line-continuation\.txt: parsed to'
  seeded_case "interview blind to truncation"         test     inject_interview_truncation_blind \
    'truncated-unterminated-fence\.txt: parsed to'
  seeded_case "interview discards trailing content"   test     inject_interview_discards_trailing \
    'content lost parsing'
  seeded_case "an event kind with no fixture"         test     inject_record_unfixtured_kind \
    'no fixture.*spurious'
  seeded_case "record substrate made optional"        test     inject_record_substrate_optional \
    'regime-missing-substrate\.jsonl: accepted as'
  seeded_case "a row that links to itself"            test     inject_record_self_link_allowed \
    'retry-of-itself\.jsonl: accepted as'
  seeded_case "record nesting left unbounded"         test     inject_record_depth_unbounded \
    'deep-nesting\.jsonl: accepted as'
  seeded_case "grounding floor made inert"            test     inject_grounded_floor_inert \
    'a lane that mostly fabricated must be rejected whole'
  seeded_case "grounding gates a judgment field"      test     inject_grounded_gates_judgment \
    'the gate touched a judgment-class field'
  seeded_case "a score with no demonstrated failure"  test     inject_grounded_undemonstrated \
    'a measurement was handed back whose instrument never failed'
  seeded_case "grounding loosened to recombination"   test     inject_grounded_loose_matching \
    'a sentence the source never said was scored as present in it'
  seeded_case "a floor of zero"                       test     inject_grounded_zero_floor \
    'a floor of zero was accepted, and every lane meets it'
  seeded_case "supersede deletes what it replaced"    test     inject_object_supersede_deletes \
    'the voided entry is still here'
  seeded_case "the reconciler stops deduping"         test     inject_object_no_dedup \
    'the same fact, wrapped differently, is the same fact'
  seeded_case "a correction that restates what it voids" test   inject_object_self_void \
    'an entry was voided by itself'
  seeded_case "a state change over a supersede link"  test     inject_object_state_overwrite \
    'a correction was erased by a later state change'
  seeded_case "a no-op patch that claims an entry"    test     inject_object_false_attribution \
    'the patch claimed entries no diff can support'
  seeded_case "a usage error that exits zero"         test     inject_cli_usage_exit \
    'a usage error must be distinguishable from a bad document'
  seeded_case "a verb wired to the wrong format"      test     inject_cli_wrong_format \
    'a valid fixture of its own format did not read'
  seeded_case "a CLI that prints no result"           test     inject_cli_silent \
    'stdout is not JSON'
  seeded_case "the regime moves under a patch"        test     inject_object_regime_mutable \
    'moved the regime the object was opened under'
  seeded_case "dedup rebinds instead of aliasing"     test     inject_object_no_alias \
    'a lane was told the object had never heard of the id it chose'
  seeded_case "a turn applied in arrival order"       test     inject_object_unsorted_turn \
    'the outcome of a turn depended on the order its patches arrived in'
  seeded_case "a self-supersede through an alias"     test     inject_object_alias_self_void \
    'was not named as one'
  seeded_case "a field kind nothing covers"           test     inject_field_kind_variant \
    'non-exhaustive patterns'
  seeded_case "a subshell read as a group"            test     inject_shell_subshell_as_group \
    'the bracketed list was not read as a subshell'
  seeded_case "a stderr pipe read as a plain pipe"    test     inject_shell_stderr_pipe_flat \
    'did not become the duplication it abbreviates'
  seeded_case "an expanding word reported literal"    test     inject_shell_expansion_literal \
    'a word the shell would expand was reported literal'
  seeded_case "an empty payload read as absent"       test     inject_record_empty_payload_dropped \
    'an empty answer is a recorded answer, not a missing one'
  seeded_case "a negative zero decimal accepted"      test     inject_record_negative_zero_decimal \
    'was constructed, and the grammar would not read it back'
  seeded_case "the producer read as the last command" test     inject_shell_producer_is_last_written \
    'produces its output with'
  seeded_case "an operator dropped from the table"    test     inject_shell_operator_table_row_dropped \
    'the table gained or lost an operator'
  seeded_case "a stripping heredoc that keeps tabs"   test     inject_shell_heredoc_strip_keeps_tabs \
    'did not strip the tabs the shell strips'
  seeded_case "the command word read as an operand"   test     inject_shell_command_word_is_an_operand \
    'the command word is not one of its own operands'
  seeded_case "a regimen float rendered as a string"  test     inject_regimen_float_as_a_string \
    'a float projected as something other than a decimal'
  seeded_case "a table header that opens nothing"     test     inject_regimen_table_scope_flattened \
    'its keys are not the document.s'
  seeded_case "two tables of one name accepted"       test     inject_regimen_table_collision_unchecked \
    'a table opened twice was not refused'
  seeded_case "the float rule widened past the record" test    inject_regimen_float_rule_widened \
    'the regimen grammar and the record.s decimal disagree'
  seeded_case "a summary the rows do not carry"       recompute inject_recompute_summary_not_derived \
    'the report does not re-derive'
  seeded_case "a results directory declaring no kind" recompute inject_recompute_kind_undeclared \
    '0 recomputed, 0 declared historical, 1 undeclared'
  seeded_case "a consumed digest gone stale"          results  inject_results_consumed_digest_stale \
    'but the committed file hashes to'
  seeded_case "an injection that changes nothing"     injections inject_inert_injection \
    'inject_that_changes_nothing'
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
  seeded_case "a check no workflow runs"              ci       inject_ci \
    'has no owner in check-owners\.tsv'
  seeded_case "parity drifts from what is proven"     parity   inject_parity \
    'which verify\.sh does not prove'
  seeded_case "a forbidden id in a commit message"    history  inject_history \
    'hygiene: internal-ticket-id:'
  seeded_case "history with an undeterminable base"   history  inject_history_no_base \
    'an undeterminable base is a failure, not an empty scan'
  seeded_case "an ask wired to another class's question" test inject_router_ask_class_untuned \
    'the ask does not ask its own question'
  seeded_case "a template without the imperative"     test     inject_router_ask_imperative_dropped \
    'the ask does not carry the fork-local imperative'
  seeded_case "a census that miscounts which classes fired" test inject_router_census_class_miscounted \
    'the census does not say which classes fired'
  seeded_case "a route verb answering with a hollow census" test inject_router_route_census_hollow \
    'where the drive spends'
  seeded_case "a table row that can never fire"       test     inject_router_table_row_shadowed \
    'can never fire'
  seeded_case "a corpus that stops covering a class"  test     inject_router_corpus_class_uncovered \
    'call\(s\) in the corpus, fewer than'
  seeded_case "a table row skipped, not refused"      test     inject_router_table_row_skipped \
    'was skipped rather than refused'
  seeded_case "an intent taken from any lane"         test     inject_router_intent_lane_ignored \
    'did not quote back what the model said it was about to do'
  seeded_case "the first stated intent, not the last" test     inject_router_intent_first_not_last \
    'quoted an intent the model had already moved past'
  seeded_case "an intent marker lost from the table"  test     inject_router_intent_marker_lost \
    'a marker was added or lost without a sentence that reaches it'
  seeded_case "an unclassified call nobody can look up" test   inject_router_unclassified_unattributed \
    'must name the call, its turn, its tool and its word'
  seeded_case "the declared default out of the vocabulary" test inject_router_class_vocabulary_shortened \
    'left the vocabulary without leaving the tests that walk it'
  seeded_case "a quoted substitution descended into"  test     inject_mechanical_quoted_substitution \
    'a single-quoted substitution was descended into'
  seeded_case "the declared default replaced by silence" test   inject_router_unknown_silent \
    'an unknown pattern must route to the declared default, never to silence'
  seeded_case "a judgment ask released mid-turn"      test     inject_router_judgment_mid_turn \
    'a judgment ask fired in the middle of a turn'
  seeded_case "a row of the routing table lost"       test     inject_router_table_row_lost \
    'misrouted call'
  seeded_case "an unknown call routed but not recorded" test   inject_router_unclassified_silent \
    'an unknown pattern must be a typed event'
  seeded_case "a reduction claimed, not computed"     test     inject_router_reduction_claimed \
    'the reduction is not the number its own counts give'
  seeded_case "a subshell that shares the parent state" test   inject_mechanical_subshell_leaks \
    'the subshell cd leaked into the parent'
  seeded_case "a failed cd applied anyway"            test     inject_mechanical_failed_cd_applied \
    'a failed cd moved the working directory'
  seeded_case "popd on an empty stack ignored"        test     inject_mechanical_popd_empty_ignored \
    'popd on an empty stack was silently ignored'
  seeded_case "the mechanical-noun table emptied"     test     inject_mechanical_lint_table_emptied \
    'a question about a mechanical fact went unflagged'
  seeded_case "a mechanical entry sent through the gate" test  inject_mechanical_entry_grounded \
    'a mechanical entry was dropped as if it needed grounding'
  seeded_case "a cosine that forgot its second norm"  test     inject_sense_cosine_unnormalised \
    'cosine of a vector with itself was not one'
  seeded_case "contrastive scoring that ignores the negative sense" test inject_sense_contrastive_ignores_negative \
    'the contrastive score ignored the negative sense'
  seeded_case "a null whose labels are never shuffled" test   inject_sense_null_labels_unshuffled \
    'd-prime on a shuffled-label null was far from zero'
  seeded_case "a bootstrap p with no attainable floor" test   inject_sense_p_without_floor \
    'a bootstrap p-value came without its attainable floor'
  seeded_case "a metric whose failure fixture is gone" test   inject_sense_metric_fixture_removed \
    'no failure fixture, so it can never be reported'
  seeded_case "a register mislabelled at its source" test     inject_sense_register_source_mislabelled \
    'and a row says otherwise'
  seeded_case "a file in the register naming nothing"  test     inject_sense_register_unnamed_file \
    'not a declared sidecar'
  seeded_case "a mined row nobody can trace"          test     inject_sense_provenance_join_dropped \
    'a row nobody can trace was accepted'
  seeded_case "two data lines read as one"            test     inject_record_data_line_two_lines \
    'two lines were read as one, and the second was lost'
  seeded_case "controls that never look at the register" test inject_sense_controls_ignore_register \
    'a register row reached the top control and the controls passed'
  seeded_case "a bootstrap that never resamples"      test     inject_sense_bootstrap_never_resamples \
    'every resample was the observed difference'
  seeded_case "a failure reading off the worst reading" test   inject_sense_failure_reading_moved \
    'is the worst the metric can say'
  seeded_case "a pre-registration with nothing in it" test     inject_sense_pre_registration_emptied \
    'the primary endpoint is not the endpoint that was registered'
  seeded_case "a lexical gate asked about the id"     test     inject_sense_gate_reads_the_id \
    'the gate did not decide on the row'
  seeded_case "a separation over one class spread"    test     inject_sense_d_prime_unpooled \
    'd-prime was standardised by one class'
  seeded_case "a null band widened past a finding"    test     inject_sense_null_band_widened \
    'bands are not the numbers they were registered as'
  seeded_case "a reported metric that reports a constant" test inject_sense_reported_value_constant \
    'the record of a metric is not the numbers the metric produced'
  seeded_case "the mechanical lane renamed"           test     inject_mechanical_lane_renamed \
    'the lane was renamed'
  seeded_case "an option word read as a directory"    test     inject_mechanical_option_is_a_directory \
    'an option word was read as the directory it names'
  seeded_case "a flag value read as a file"           test     inject_mechanical_flag_value_is_a_file \
    'the lane read a file out of a flag.s value'
  seeded_case "an entry with the wrong verb"          test     inject_mechanical_entry_verb_swapped \
    'the entry used the wrong verb for what happened to the file'
  seeded_case "a pipeline whose members never run"    test     inject_mechanical_pipeline_skipped \
    'nothing in the pipeline ran'
  seeded_case "a write lost to a read of the same path" test   inject_mechanical_write_lost_to_a_read \
    'a write was lost to a read of the same path in the same call'

  echo
  echo "--- results fixtures, checked directly ---"
  # The linter dispatches the record verdict to diet through the resolver, so
  # it needs exactly one fresh build where the resolver will look -- the same
  # thing check_results does before it runs the linter for real.
  ( cd "${ROOT}" && build_diet ) || SELFTEST_BROKEN+=("results fixtures: diet did not build")
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
