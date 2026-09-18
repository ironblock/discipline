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

  # A fresh-instance review of #83 found this list was the reason
  # `DIET_REQUIRE_SANDBOX=1` (set in `gate-selftest.yml` so a host promising a
  # sandbox turns a missing one into a failure, not a silent refusal) never
  # reached the isolation lane's own `cargo test` inside `--selftest` -- every
  # lane case runs through this script, and an allowlist that omits a
  # variable admits nothing it does not name. Says nothing about repository
  # or CI identity, so it belongs beside the toolchain variables above it.
  DIET_REQUIRE_SANDBOX

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

# HOME has to survive -- cargo and rustup need it -- and git reads
# $HOME/.gitconfig through it. That is the allowlist leaking behaviour through
# a file handle rather than a variable: a contributor with `commit.gpgsign` set
# would have sandbox commits attempt to sign and fail, so the gate's verdict
# would depend on whose machine ran it. Same family as the GITHUB_* incident.
#
# FORCED, not allowlisted: these are set regardless of the ambient value,
# because the point is that no ambient value reaches git.
keep+=(GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null)

exec env -i "${keep[@]}" "$@"
