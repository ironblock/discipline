#!/usr/bin/env bash
# Build `diet-drive` and place it where Tauri's externalBin looks for it:
# binaries/diet-drive-<host triple> (#719). Run from anywhere.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
triple="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release --manifest-path "$root/Cargo.toml" -p discipline-diet --bin diet-drive
mkdir -p "$here/binaries"
cp "$root/target/release/diet-drive" "$here/binaries/diet-drive-$triple"
echo "sidecar: binaries/diet-drive-$triple"
