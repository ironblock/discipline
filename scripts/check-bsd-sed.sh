#!/usr/bin/env bash
# Every injection changes the tree under BSD `sed` semantics too (#75).
#
# `sed -i` is not portable: GNU takes `-i`'s suffix attached to the flag, BSD
# takes it as the NEXT ARGUMENT, so GNU's `sed -i 's/a/b/' f` is, to BSD, the
# suffix `s/a/b/` with `f` as the script. Twenty-eight injections were inert
# on a Mac while CI was green for that reason (#50). The structural rule in
# scripts/check-injections.py keeps `sed` out of injection bodies, so
# `edit_in_place` is the one spelling -- and this is what proves that one
# spelling portable BY EXECUTION: it runs the same applier with
# scripts/bsd-sed/sed, a shim reproducing that one incompatibility, first on
# PATH. An injection inert under the shim is the defect.
#
# Usage: check-bsd-sed.sh [ROOT [--only inject_NAME]]
#        ROOT defaults to this script's checkout; `--only` is handed to the
#        applier, as `verify.sh --only bsd --scope inject_NAME` does (#112).
#
# Exit 0 if every injection changes the tree under the shim, 1 if any is
# inert, 2 if the shim could not be put in force: unreadable, not executable,
# no real sed for it to delegate to, not first on PATH, or not BSD-shaped
# when tried. A check that silently ran under GNU semantics would pass
# vacuously, which is the class this repository refuses.

set -u

readonly EXIT_BROKEN=2

root="${1:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)}"
[ $# -gt 0 ] && shift
# ABSOLUTE, because the applier runs each injection after `cd`-ing into its
# box: a relative PATH entry would point nowhere there, `sed` would resolve to
# the real one, and every injection would be graded under GNU semantics while
# the checks below, run from here, all passed (#260's review).
given="$root"
root="$(cd -- "$given" 2> /dev/null && pwd)" || {
  echo "check-bsd-sed: '${given}' is not a directory" >&2
  exit 2
}
shim_dir="${root}/scripts/bsd-sed"
shim="${shim_dir}/sed"

broken() {
  echo "check-bsd-sed: $1; the injections were not run under BSD semantics" >&2
  exit "$EXIT_BROKEN"
}

[ -r "$shim" ] && [ -x "$shim" ] || broken "${shim} is not a readable executable"
[ -x /usr/bin/sed ] || [ -x /bin/sed ] || broken "no real sed at /usr/bin/sed or /bin/sed for the shim to delegate to"

resolved="$(PATH="${shim_dir}:${PATH}" command -v sed)"
[ "$resolved" = "$shim" ] || broken "sed resolves to ${resolved:-nothing}, not the shim"

# The shim, tried before it is trusted. Each GNU-only in-place spelling must
# leave the file as it was -- a shim that edits through one would grade every
# injection written that way under GNU semantics and call them portable --
# and the two spellings BSD itself reads must work, so a shim that does
# nothing at all cannot pass for one that refuses.
probe="$(mktemp)" || broken "no temporary file for the probe"
trap 'rm -f -- "$probe" "${probe}.orig"' EXIT
gnu_only() {
  case "$1" in
    leading) sed -i 's/a/b/' "$probe" ;;
    long) sed --in-place 's/a/b/' "$probe" ;;
    clustered) sed -s -i 's/a/b/' "$probe" ;;
    after-the-script) sed -e 's/a/b/' -i "$probe" ;;
    trailing) sed 's/a/b/' "$probe" -i ;;
  esac
}
for spelling in leading long clustered after-the-script trailing; do
  printf 'a\n' > "$probe"
  PATH="${shim_dir}:${PATH}" gnu_only "$spelling" > /dev/null 2>&1
  [ "$(cat "$probe")" = a ] || broken "the shim edited a file through GNU's ${spelling} -i spelling"
done
printf 'a\n' > "$probe"
[ "$(PATH="${shim_dir}:${PATH}" sed 's/a/b/' "$probe" 2> /dev/null)" = b ] \
  || broken "the shim does not run an ordinary sed"
PATH="${shim_dir}:${PATH}" sed -i .orig 's/a/b/' "$probe" > /dev/null 2>&1
[ "$(cat "$probe")" = b ] && [ "$(cat "${probe}.orig" 2> /dev/null)" = a ] \
  || broken "the shim does not edit in place through BSD's own spelling, sed -i SUFFIX"

rc=0
PATH="${shim_dir}:${PATH}" python3 "${root}/scripts/check-injections.py" "$root" "$@" || rc=$?
if [ "$rc" -eq 1 ]; then
  echo "check-bsd-sed: the injection(s) above are inert under BSD sed semantics" >&2
fi
exit "$rc"
