#!/usr/bin/env bash
# Gate 0 for a TabbyAPI cells directory (#393): the re-derivation is substrates/admission/depth-probe/tabby_cells.py check.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -B "$here/../../depth-probe/tabby_cells.py" check "$here"
