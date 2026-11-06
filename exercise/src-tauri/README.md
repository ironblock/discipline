# The native shell (#719)

The same web surface in a native window: the webview loads the bundle `pnpm build:drive` writes (`../dist/app`), and `serve` runs as the shell's sidecar, or, by a setting, the shell drives a `serve` running elsewhere. No feature lives only here.

This crate is its own Cargo workspace, outside the repository's, so CI's cargo never needs a webview's libraries. CI does not build it: it is built and checked by hand, as below.

## Pins

| what | version | published |
|---|---|---|
| `tauri` | `=2.12.4` | 2026-10-14 |
| `tauri-build` | `=2.7.3` | 2026-10-14 |
| `tauri-plugin-shell` | `=2.5.0` | 2026-10-10 |
| `tauri-plugin-notification` | `=2.7.0` | 2026-10-14 |
| `tauri-plugin-opener` | `=2.7.2` | 2026-10-14 |
| `ctrlc` (`termination`) | `=3.5.2` | 2026-02-10 |
| `base64` | `=0.23.1` | 2026-08-04 |
| `security-framework` (macOS only) | `=3.7.0` | 2026-02-20 |
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

- `EXERCISE_DRIVE`: a running `serve`'s URL, https, or http on `127.0.0.1` or `localhost`: the page sends the credential there, and the CSP connects nowhere else. That `serve` must admit the webview's origin: `--allow-origin tauri://localhost` (macOS and Linux).
- `EXERCISE_PROFILE`: a `serve` profile in the per-user config (`diet-drive serve --profile`), run as the sidecar with that origin admitted.
- `settings.json` in the app's config directory (`~/Library/Application Support/app.discipline.exercise/` on macOS): `{"drive": "<url>"}` or `{"profile": "<name>"}`, and optionally `"auth_file"` or `"auth": "keychain"`.

## The credential

A `serve` with `--auth-file` (which a regimen that runs commands requires) is driven with the same file: `EXERCISE_AUTH_FILE`, or settings.json's `auth_file`. The sidecar is started with it, and the page asks the shell for the `Authorization` value over IPC (`drive_authorization`, granted to the `main` window alone in `capabilities/default.json`); it is never in a URL or the bundle (`src/drive/credential.ts`).

### From the keychain (macOS)

With a profile, the credential can live in the login keychain instead of a file: set `EXERCISE_AUTH=keychain`, or settings.json's `"auth": "keychain"`. The shell reads the generic password with service `app.discipline.exercise` and account the profile, gives it to the sidecar on its stdin (`serve --auth-stdin`), and gives the page the same value over IPC. No file is written. The keychain and an auth file are never named together.

The shell never writes the keychain. Add the item once, as `user:password`; `security` prompts for it, so it is in no shell history:

```
security add-generic-password -s app.discipline.exercise -a <profile> -w
```

macOS asks once to allow the app's read of the item. A `drive` URL names no profile, so it takes an auth file.

The page renders model output and holds a credential, so its CSP (`tauri.conf.json`) runs no script but the bundle's own, and connects only to the IPC, loopback and https. `./csp-probe.sh` is its recorded check: it builds the shell over `csp-probe/` and lists which beacons ran; `--no-csp` is the control. Expected with the CSP: `bundle-script-ran`, `eval-refused`, `inline-script-ran` (a script in the bundle's own HTML, which Tauri hashes into the CSP when it builds; the page has none). Never `injected-script-ran`, `inline-handler-ran` or `eval-ran`.

The sidecar is killed when the app exits, by a quit or by `SIGTERM`, `SIGINT` or `SIGHUP`; a `SIGKILL` leaves it running.

## Notifications

When the session begins to wait on the operator, the page asks the shell to tell them (`src/drive/notices.ts`), and the shell sends a system notification only while its window is not focused. The command, `notify`, is granted to the `main` window alone, and none of the notification plugin's own commands is. A click on a notification brings the window forward. The moments are those the page already shows:

- a command waiting on approval: *a command waits on you*, with the command's first line;
- a meditation held for the operator (#150's gate): *the meditation is held*;
- the model's phase proposal (#651): *the model proposes a phase move*, with its from and to.

Each is told once, as it begins.

## Links

A link in model output opens in the system's browser when it is http, https or mailto (`on_new_window`, the app audit); anything else, and any new window, is refused. The opener is called from Rust alone: the page is granted none of its commands.

A profile whose serve listens anywhere but `127.0.0.1` or `localhost` is refused at start, since the page's CSP connects nowhere else.

## Attachments

The composer's **attach** button opens macOS's own file panel: the webview answers the page's file input itself (`wry`'s `NSOpenPanel`, #1048). That panel does not filter to PNG, so any file can be chosen; `POST /files` refuses anything else, and the page says so.
