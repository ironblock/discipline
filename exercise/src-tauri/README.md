# The native shell (#719)

The same web surface in a native window: the webview loads the bundle `pnpm build:drive` writes (`../dist/app`), and `serve` runs as the shell's sidecar, or, by a setting, the shell drives a `serve` running elsewhere. No feature lives only here.

This crate is its own Cargo workspace, outside the repository's, so CI's cargo never needs a webview's libraries. CI does not build it: it is built and checked by hand, as below.

## Pins

| what | version | published |
|---|---|---|
| `tauri` | `=2.12.4` | 2026-10-14 |
| `tauri-build` | `=2.7.3` | 2026-10-14 |
| `tauri-plugin-shell` | `=2.5.0` | 2026-10-10 |
| `ctrlc` (`termination`) | `=3.5.2` | 2026-02-10 |
| `tauri-cli` (`cargo install tauri-cli --version 2.12.3 --locked`) | `2.12.3` | 2026-10-14 |

`Cargo.lock` pins the rest. Each needs Rust 1.90 or newer.

## Build

From the repository root:

```
COREPACK_ENABLE_DOWNLOAD_PROMPT=0 npx --yes pnpm@11.20.0 --dir exercise install --frozen-lockfile
npx --yes pnpm@11.20.0 --dir exercise gate
npx --yes pnpm@11.20.0 --dir exercise build:drive
exercise/src-tauri/sidecar.sh
cd exercise/src-tauri && cargo tauri build --bundles app
```

`sidecar.sh` builds `diet-drive` and places it at `binaries/diet-drive-<host triple>`, where Tauri's `externalBin` looks. The app lands under `target/release/bundle/`.

## Where it drives

The first of these that is set:

- `EXERCISE_DRIVE`: a running `serve`'s URL. That `serve` must admit the webview's origin: `--allow-origin tauri://localhost` (macOS and Linux).
- `EXERCISE_PROFILE`: a `serve` profile in the per-user config (`diet-drive serve --profile`), run as the sidecar with that origin admitted.
- `settings.json` in the app's config directory (`~/Library/Application Support/app.discipline.exercise/` on macOS): `{"drive": "<url>"}` or `{"profile": "<name>"}`.

The sidecar is killed when the app exits, by a quit or by `SIGTERM`, `SIGINT` or `SIGHUP`; a `SIGKILL` leaves it running.

## Not yet

- A `serve` that asks for an auth file (`--auth-file`, which a regimen that runs commands requires) is not driven yet. The page sends no credentials of its own; the browser surface gets them from Vite's proxy. The shell will hold them in the keychain and add them (#719's keychain item).
- The native file picker and notifications (#719), each its own change.
