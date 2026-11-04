//! The native shell (#719): the surface's built bundle in a webview, and
//! `serve` as its sidecar -- or, by a setting, a `serve` running elsewhere.
//!
//! Nothing here is a feature of its own: the page is the same bundle the
//! browser loads (`pnpm build:drive`, `../dist/app`), told where its drive is
//! by `?drive=<url>` (`src/drive/base.ts`).
//!
//! Where the drive is, first found of:
//! - `EXERCISE_DRIVE`: a running `serve`'s URL, `http(s)://...`;
//! - `EXERCISE_PROFILE`: a `serve` profile (`diet-drive serve --profile`), run
//!   as the sidecar;
//! - `settings.json` in the app's config directory: `{"drive": "<url>"}` or
//!   `{"profile": "<name>"}`, exactly one of them.
//!
//! The sidecar admits the webview's origin (`--allow-origin`), which `serve`
//! answers across origins (#719's CORS), and it is killed when the app exits,
//! by a quit or by a termination signal.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

/// The webview's origin on macOS and Linux, which the sidecar admits.
const ORIGIN: &str = "tauri://localhost";

/// Where the drive is.
enum Drive {
    /// A `serve` running elsewhere, by its URL.
    Remote(String),
    /// A `serve` profile, run as the sidecar.
    Profile(String),
}

/// The sidecar, held so it is killed when the app exits.
struct Sidecar(Mutex<Option<CommandChild>>);

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecar(Mutex::new(None)))
        .setup(|app| {
            let handle = app.handle().clone();
            // A SIGTERM, SIGINT or SIGHUP exits as a quit does, so the
            // sidecar is killed then too; only a SIGKILL leaves it running.
            let quit = handle.clone();
            ctrlc::set_handler(move || quit.exit(0)).map_err(|why| why.to_string())?;
            let drive = match setting(&handle)? {
                Drive::Remote(url) => url,
                Drive::Profile(profile) => {
                    let (url, child) = sidecar(&handle, &profile)?;
                    if let Ok(mut held) = handle.state::<Sidecar>().0.lock() {
                        *held = Some(child);
                    }
                    url
                }
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page(&drive)?.into()))
                .title("exercise")
                .inner_size(1280.0, 860.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|why| {
            eprintln!("exercise: {why}");
            std::process::exit(1);
        });
    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Ok(mut held) = handle.state::<Sidecar>().0.lock()
            && let Some(child) = held.take()
        {
            let _ = child.kill();
        }
    });
}

/// Where the drive is, as the module's doc orders the places it is named.
fn setting(app: &AppHandle) -> Result<Drive, String> {
    if let Ok(url) = std::env::var("EXERCISE_DRIVE") {
        return remote(url);
    }
    if let Ok(profile) = std::env::var("EXERCISE_PROFILE") {
        return Ok(Drive::Profile(profile));
    }
    let path = app
        .path()
        .app_config_dir()
        .map_err(|why| why.to_string())?
        .join("settings.json");
    let text = std::fs::read_to_string(&path).map_err(|why| {
        format!(
            "{}: {why}; name a `drive` URL or a serve `profile` there, or set EXERCISE_DRIVE or EXERCISE_PROFILE",
            path.display()
        )
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))?;
    let named = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    match (named("drive"), named("profile")) {
        (Some(url), None) => remote(url),
        (None, Some(profile)) => Ok(Drive::Profile(profile)),
        _ => Err(format!(
            "{}: name exactly one of `drive` or `profile`",
            path.display()
        )),
    }
}

/// A remote drive's URL, an absolute http(s) one, as the page takes it.
fn remote(url: String) -> Result<Drive, String> {
    match Url::parse(&url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => Ok(Drive::Remote(url)),
        _ => Err(format!("`{url}` is no http(s) URL")),
    }
}

/// `serve --profile <profile>` as the sidecar, admitting the webview's
/// origin; its URL, from the first line it prints, and the process.
fn sidecar(app: &AppHandle, profile: &str) -> Result<(String, CommandChild), String> {
    let (mut events, child) = app
        .shell()
        .sidecar("diet-drive")
        .map_err(|why| why.to_string())?
        .args(["serve", "--profile", profile, "--allow-origin", ORIGIN])
        .spawn()
        .map_err(|why| why.to_string())?;
    while let Some(event) = events.blocking_recv() {
        match event {
            CommandEvent::Stdout(line) => {
                let first: serde_json::Value = serde_json::from_slice(&line)
                    .map_err(|why| format!("serve's first line is no JSON: {why}"))?;
                let Some(listening) = first.get("listening").and_then(serde_json::Value::as_str)
                else {
                    return Err(format!("serve's first line names no address: {first}"));
                };
                let url = format!("http://{listening}/");
                // What serve says after its first line goes to the shell's
                // own stderr, so the channel never fills.
                tauri::async_runtime::spawn(async move {
                    while let Some(event) = events.recv().await {
                        if let CommandEvent::Stderr(line) | CommandEvent::Stdout(line) = event {
                            eprintln!("serve: {}", String::from_utf8_lossy(&line).trim_end());
                        }
                    }
                });
                return Ok((url, child));
            }
            CommandEvent::Stderr(line) => {
                eprintln!("serve: {}", String::from_utf8_lossy(&line).trim_end());
            }
            CommandEvent::Terminated(ended) => {
                return Err(format!("serve exited before it listened: {:?}", ended.code));
            }
            _ => {}
        }
    }
    Err("serve's output closed before it listened".to_owned())
}

/// The bundle's page, told its drive by `?drive=<url>`.
fn page(drive: &str) -> Result<String, String> {
    let mut url = Url::parse("tauri://localhost/index.html").map_err(|why| why.to_string())?;
    url.query_pairs_mut().append_pair("drive", drive);
    Ok(format!("index.html?{}", url.query().unwrap_or_default()))
}
