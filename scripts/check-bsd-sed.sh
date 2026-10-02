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
# Usage: check-bsd-sed.sh [ROOT]     (default: this script's checkout)
#
# Exit 0 if every injection changes the tree under the shim, 1 if any is
# inert, 2 if the shim could not be put in force: unreadable, not executable,
# no real sed for it to delegate to, not first on PATH, or not BSD-shaped
# when tried. A check that silently ran under GNU semantics would pass
# vacuously, which is the class this repository refuses.

set -u

readonly EXIT_BROKEN=2

root="${1:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)}"
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

# The shim's one difference, tried before it is trusted: GNU's in-place
# spelling must leave the file as it was. A shim that edits it would grade
# every injection under GNU semantics and call them portable.
probe="$(mktemp)" || broken "no temporary file for the probe"
printf 'a\n' > "$probe"
PATH="${shim_dir}:${PATH}" sed -i 's/a/b/' "$probe" > /dev/null 2>&1
if [ "$(cat "$probe")" != a ]; then
  rm -f -- "$probe"
  broken "the shim edited a file through GNU's -i spelling"
fi
rm -f -- "$probe"

rc=0
PATH="${shim_dir}:${PATH}" python3 "${root}/scripts/check-injections.py" "$root" || rc=$?
if [ "$rc" -eq 1 ]; then
  echo "check-bsd-sed: the injection(s) above are inert under BSD sed semantics" >&2
fi
exit "$rc"
