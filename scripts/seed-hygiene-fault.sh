#!/usr/bin/env bash
#
# Materialise a tree that scripts/hygiene.sh must reject, one directory per
# forbidden class, so that every pattern in hygiene-patterns.tsv can be shown
# catching something. A pattern that has never caught anything is a guess.
#
#   seed-hygiene-fault.sh DIR
#
# Every forbidden string is assembled here from fragments at run time. No file
# in this repository contains a complete one, which is why this script does not
# itself trip the gate it exists to test.
#
# EACH CLASS GETS THREE FILES, and the third is the one that was missing:
#
#   LABEL.txt           the prose form
#   LABEL.jsonl         the same lines inside a JSON string, escaped as a
#                       captured log carries them
#   LABEL.welded.jsonl  the forbidden FRAGMENT alone, immediately after an
#                       escaped newline inside a JSON string
#
# A pattern that guards prose and not logs passes half its own test and is a
# guard over the half where the artefacts are not. That is why the second file
# exists.
#
# THE THIRD EXISTS BECAUSE THE SECOND DID NOT DO WHAT IT CLAIMED. `verify.sh`
# said the escaped twin was what keeps the decoded view load-bearing.
# Measured: with the mirror removed, 13 of 14 classes still fired on their
# twin -- because `json.dumps` puts the escaped newline at the END of the
# line, so nothing is welded to the forbidden string's left and the raw bytes
# match it anyway. The twin proved the pattern; it did not prove the view.
#
# The welded file puts the escape immediately in front of the fragment, which
# is the shape the whole decoder exists for: in the raw bytes the character to
# its left is the `n` of `\n`, so a pattern needing a boundary there walks
# past it. Measured again, on the welded file:
#
#   with the mirror removed, these 4 go quiet   private-ipv4
#                                               internal-ticket-id
#                                               openai-api-key
#                                               assigned-secret
#   these 10 still fire, and CANNOT be hidden this way
#
# The ten are not a gap. A pattern anchored on a character that is never a
# token character -- `/home/`, `-----BEGIN `, `C:\`, `sk-ant-` -- keeps its
# own left boundary, so welding a letter in front of it hides nothing. Only a
# pattern whose match may BEGIN with an alphanumeric can be welded shut, and
# those four are exactly the ones that can. Which is the honest form of the
# claim the old comment made too broadly.
#
# CALL CONVENTION: pairs of LINE and FRAGMENT, where FRAGMENT is the forbidden
# substring inside LINE. Passing a fragment the line does not contain is an
# error rather than a weaker fixture -- a welded file built from the wrong
# text proves nothing and looks identical to one that proves everything.

set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: seed-hygiene-fault.sh DIR" >&2; exit 2; }
readonly DEST="$1"
mkdir -p "$DEST"

seed() {
  local label="$1"; shift
  [ $(( $# % 2 )) -eq 0 ] || {
    echo "seed ${label}: arguments are LINE FRAGMENT pairs, got $#" >&2
    exit 2
  }
  mkdir -p "${DEST}/${label}"
  local -a lines=() frags=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      *"$2"*) ;;
      *)
        # A fragment the line does not contain would build a welded file out
        # of text the class never carried: a fixture that proves nothing and
        # is indistinguishable from one that proves everything.
        echo "seed ${label}: '$2' is not inside '$1'" >&2
        exit 2
        ;;
    esac
    lines+=("$1"); frags+=("$2"); shift 2
  done

  printf '%s\n' "${lines[@]}" > "${DEST}/${label}/${label}.txt"
  # The escaped twin, and the welded one. Written by python3 rather than by
  # hand because some of these lines carry a backslash and one carries a
  # quote, and a JSON string that escapes them wrongly is a fixture that
  # proves the escaping is broken rather than that the pattern reaches
  # through it.
  printf '%s\n' "${lines[@]}" | python3 -c '
import json, sys
for line in sys.stdin.read().splitlines():
    print(json.dumps({"stdout": line + "\n"}))
' > "${DEST}/${label}/${label}.jsonl"
  printf '%s\n' "${frags[@]}" | python3 -c '
import json, sys
for fragment in sys.stdin.read().splitlines():
    # `ran\n` in FRONT of the fragment, which is the whole point: in the bytes
    # on disk the character to its left is the `n` of the escape.
    print(json.dumps({"stdout": "ran\n" + fragment + " regressed\n"}))
' > "${DEST}/${label}/${label}.welded.jsonl"
}

seed private-ipv4 \
  "$(printf 'host = %s.%s' '192.168' '1.10')"        "$(printf '%s.%s' '192.168' '1.10')" \
  "$(printf 'gateway = %s.%s' '10' '0.0.1')"         "$(printf '%s.%s' '10' '0.0.1')" \
  "$(printf 'peer = %s.%s' '172.16' '4.9')"          "$(printf '%s.%s' '172.16' '4.9')"

seed internal-hostname \
  "$(printf 'ssh %s%s' 'build-box' '.local')"        "$(printf '%s%s' 'build-box' '.local')" \
  "$(printf 'proxy: %s%s' 'artifacts' '.internal')"  "$(printf '%s%s' 'artifacts' '.internal')"

seed personal-home-path \
  "$(printf 'cargo run --manifest-path %s%s' '/home' '/someone/scratch/Cargo.toml')" \
    "$(printf '%s%s' '/home' '/someone/scratch/Cargo.toml')" \
  "$(printf 'log: %s%s' '/Users' '/someone/Library/Logs/run.log')" \
    "$(printf '%s%s' '/Users' '/someone/Library/Logs/run.log')" \
  "$(printf 'the build ran under %s%s and failed' '/home' '/someone')" \
    "$(printf '%s%s' '/home' '/someone')"

# BOTH FORMS. The row was anchored on a `/private` prefix -- macOS only --
# and a Linux seat's scratchpad has none, which is to say the row missed the
# thing it is named for on the platform CI runs on. The prefix is optional
# now, and the Linux line below is why that is a claim rather than a hope.
#
# Note the spelling of the comment above. Its first draft wrote the Linux path
# out whole, and the widened row promptly caught it -- in the file that
# widened it. No file here may contain a complete forbidden string; that is
# why every seeded line is assembled from fragments at run time.
seed session-scratchpad-path \
  "$(printf 'scratch = %s%s' '/private/tmp/claude-' '501/notes.md')" \
    "$(printf '%s%s' '/private/tmp/claude-' '501/notes.md')" \
  "$(printf 'cd %s%s' '/private/tmp/claude-' '77/x/')" \
    "$(printf '%s%s' '/private/tmp/claude-' '77/x/')" \
  "$(printf 'wrote %s%s' '/tmp/claude-' '0/session/scratchpad/notes.md')" \
    "$(printf '%s%s' '/tmp/claude-' '0/session/scratchpad/notes.md')"

seed ssh-user-at-host \
  "$(printf 'scp %s%s:/var/log/run.log .' 'someone@' 'box.example.net')" \
    "$(printf '%s%s:' 'someone@' 'box.example.net')" \
  "$(printf '%s %s%s' 'ssh' 'someone@' 'box.example.net')" \
    "$(printf '%s %s%s' 'ssh' 'someone@' 'box.example.net')"

seed windows-user-path \
  "$(printf 'path = %s%s' 'C:' '\Users\someone\AppData')" \
    "$(printf '%s%s' 'C:' '\Users\someone')"

seed internal-ticket-id \
  "$(printf 'see %s%s for the rollout plan' 'DIE' '-4172')" \
    "$(printf '%s%s' 'DIE' '-4172')" \
  "$(printf 'arm %s%s was the one that regressed' 'die' '45')" \
    "$(printf '%s%s' 'die' '45')"

seed aws-access-key-id \
  "$(printf 'aws_access_key_id = %s%s' 'AKIA' 'EXAMPLEKEYID1234')" \
    "$(printf '%s%s' 'AKIA' 'EXAMPLEKEYID1234')"

seed github-token \
  "$(printf 'GH_TOKEN=%s%s' 'ghp_' '0123456789abcdefghijklmnopqrstuvwxyz')" \
    "$(printf '%s%s' 'ghp_' '0123456789abcdefghijklmnopqrstuvwxyz')"

seed slack-token \
  "$(printf 'SLACK=%s%s' 'xoxb' '-0123456789-abcdefghij')" \
    "$(printf '%s%s' 'xoxb' '-0123456789-abcdefghij')"

seed private-key-block \
  "$(printf '%s%s' '-----BEGIN ' 'PRIVATE KEY-----')" \
    "$(printf '%s%s' '-----BEGIN ' 'PRIVATE KEY-----')" \
  "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ" \
    "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ" \
  "$(printf '%s%s' '-----END ' 'PRIVATE KEY-----')" \
    "$(printf '%s%s' '-----END ' 'PRIVATE KEY-----')"

seed anthropic-api-key \
  "$(printf 'ANTHROPIC=%s%s' 'sk-ant-' 'api03-0123456789abcdefghijklmn')" \
    "$(printf '%s%s' 'sk-ant-' 'api03-0123456789abcdefghijklmn')"

seed openai-api-key \
  "$(printf 'OPENAI=%s%s' 'sk-' '0123456789abcdefghijklmnopqrstuvwxyz')" \
    "$(printf '%s%s' 'sk-' '0123456789abcdefghijklmnopqrstuvwxyz')"

seed assigned-secret \
  "$(printf '%s = "%s"' 'password' 'correcthorsebatterystaple')" \
    "$(printf '%s = "%s"' 'password' 'correcthorsebatterystaple')"

echo "seeded $(find "$DEST" -mindepth 1 -maxdepth 1 -type d | wc -l) fault directories in ${DEST}"
