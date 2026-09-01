#!/usr/bin/env bash
#
# Run a command with only an allowlisted environment.
#
#   hermetic.sh COMMAND [ARG...]
#   hermetic.sh env VAR=VALUE COMMAND [ARG...]
#
# A selftest sandbox is a throwaway checkout, not this repository's build. If
# it can see the ambient environment, a check that reads that environment
# behaves differently inside the sandbox than a contributor would see -- and
# that is not hypothetical: `history` read the REAL repository's event payload
# from inside a sandbox and failed for a reason unrelated to its seeded fault,
# reading RED while proving nothing.
#
# An ALLOWLIST, deliberately, not a blocklist. A blocklist silently admits
# every variable nobody thought of; adding to a list is a visible decision.
# Each entry below is here for a stated reason, and nothing else survives.

set -euo pipefail

[ "$#" -ge 1 ] || { echo "usage: hermetic.sh COMMAND [ARG...]" >&2; exit 2; }

# Toolchain and locale: without these, cargo and python cannot run at all, and
# nothing about them tells a check which repository or CI system it is in.
readonly ALLOWED=(
  PATH HOME TMPDIR TERM
  LANG LC_ALL LC_CTYPE
  CARGO_HOME RUSTUP_HOME RUSTUP_TOOLCHAIN CARGO_TARGET_DIR

  # Network plumbing. Building in a sandbox fetches crates, which on a
  # proxied host needs these. They say nothing about repository identity.
  HTTP_PROXY HTTPS_PROXY NO_PROXY http_proxy https_proxy no_proxy
  SSL_CERT_FILE SSL_CERT_DIR CURL_CA_BUNDLE CARGO_HTTP_CAINFO
  REQUESTS_CA_BUNDLE NODE_EXTRA_CA_CERTS
)

keep=()
for name in "${ALLOWED[@]}"; do
  if [ -n "${!name-}" ]; then
    keep+=("${name}=${!name}")
  fi
done

exec env -i "${keep[@]}" "$@"
