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

readonly CHECKS=(fmt clippy test library results regimen metadata hygiene pages ci history parity)

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
inject_cli_usage_exit() {
  sed -i 's|^const EXIT_USAGE: u8 = 2;$|const EXIT_USAGE: u8 = 0;|' diet/src/bin/diet.rs
}

inject_cli_wrong_format() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/bin/diet.rs")
source = path.read_text(encoding="utf-8")
old = '    ("parse-interview", "interview"),\n    ("check-record", "record"),'
new = '    ("parse-interview", "record"),\n    ("check-record", "interview"),'
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
