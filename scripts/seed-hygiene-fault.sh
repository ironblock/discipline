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

set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: seed-hygiene-fault.sh DIR" >&2; exit 2; }
readonly DEST="$1"
mkdir -p "$DEST"

seed() {
  local label="$1"; shift
  mkdir -p "${DEST}/${label}"
  printf '%s\n' "$@" > "${DEST}/${label}/${label}.txt"
}

seed private-ipv4 \
  "$(printf 'host = %s.%s' '192.168' '1.10')" \
  "$(printf 'gateway = %s.%s' '10' '0.0.1')" \
  "$(printf 'peer = %s.%s' '172.16' '4.9')"

seed internal-hostname \
  "$(printf 'ssh %s%s' 'build-box' '.local')" \
  "$(printf 'proxy: %s%s' 'artifacts' '.internal')"

seed personal-home-path \
  "$(printf 'cargo run --manifest-path %s%s' '/home' '/someone/scratch/Cargo.toml')" \
  "$(printf 'log: %s%s' '/Users' '/someone/Library/Logs/run.log')"

seed windows-user-path \
  "$(printf 'path = %s%s' 'C:' '\Users\someone\AppData')"

seed internal-ticket-id \
  "$(printf 'see %s%s for the rollout plan' 'DIE' '-4172')"

seed aws-access-key-id \
  "$(printf 'aws_access_key_id = %s%s' 'AKIA' 'EXAMPLEKEYID1234')"

seed github-token \
  "$(printf 'GH_TOKEN=%s%s' 'ghp_' '0123456789abcdefghijklmnopqrstuvwxyz')"

seed slack-token \
  "$(printf 'SLACK=%s%s' 'xoxb' '-0123456789-abcdefghij')"

seed private-key-block \
  "$(printf '%s%s' '-----BEGIN ' 'PRIVATE KEY-----')" \
  "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ" \
  "$(printf '%s%s' '-----END ' 'PRIVATE KEY-----')"

seed anthropic-api-key \
  "$(printf 'ANTHROPIC=%s%s' 'sk-ant-' 'api03-0123456789abcdefghijklmn')"

seed openai-api-key \
  "$(printf 'OPENAI=%s%s' 'sk-' '0123456789abcdefghijklmnopqrstuvwxyz')"

seed assigned-secret \
  "$(printf '%s = "%s"' 'password' 'correcthorsebatterystaple')"

echo "seeded $(find "$DEST" -mindepth 1 -maxdepth 1 -type d | wc -l) fault directories in ${DEST}"
