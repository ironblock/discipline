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
//! A drive that asks for a credential (`serve --auth-file`, which a regimen
//! that runs commands requires) is given one by the auth file the shell is
//! started with: `EXERCISE_AUTH_FILE`, or settings.json's `auth_file`. The
//! sidecar is started with it. Or, on macOS, by the login keychain (#1047):
//! `EXERCISE_AUTH=keychain`, or settings.json's `"auth": "keychain"`, reads
//! the generic password of service `app.discipline.exercise` and account the
//! profile, and gives it to the sidecar on its stdin (`serve --auth-stdin`);
//! no file is written, and the shell never writes the keychain. Either way
//! the page asks for the `Authorization` value
//! over IPC (`drive_authorization`, granted to the `main` window alone), never
//! in its URL (`src/drive/credential.ts`). The page renders model output, so its
//! CSP (`tauri.conf.json`) runs no script but the bundle's own.
//!
//! The page tells the shell when the session begins to wait on the operator
//! (`src/drive/notices.ts`, #1049): `notify`, granted to the `main` window
//! alone, sends a system notification while that window is not focused, and
//! a click on one brings the window forward.
//!
//! The sidecar admits the webview's origin (`--allow-origin`), which `serve`
//! answers across origins (#719's CORS), and it is killed when the app exits,
//! by a quit or by a termination signal.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;

use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;
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

/// The sidecar, held so its session is ended when the app exits.
struct Sidecar(Mutex<Option<Held>>);

/// A running sidecar: the process, where it listens, and whether it has
/// exited, set when its output says so.
struct Held {
    child: CommandChild,
    listening: String,
    exited: Arc<(Mutex<bool>, Condvar)>,
}

/// How long a quit waits for serve to end its session and exit before it is
/// killed (#1050's audit, finding 5).
const QUIT_WAIT: Duration = Duration::from_secs(10);

/// The `Authorization` value for the drive, when it asks for one.
struct Credential(Option<String>);

/// The keychain item's service; its account is the profile.
const KEYCHAIN_SERVICE: &str = "app.discipline.exercise";

/// Where the drive's credential comes from.
enum Auth {
    /// An auth file, `user:password`, named to the sidecar.
    File(String),
    /// The login keychain's item for the profile, piped to the sidecar.
    Keychain,
}

/// The drive's `Authorization` value, for the page's own window alone.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // a command takes its state by value
fn drive_authorization(credential: tauri::State<'_, Credential>) -> Option<String> {
    credential.0.clone()
}

/// A system notification of `title` and `body`, from the page's own window
/// while it is not focused; a focused window is already in front of the
/// operator, and is told nothing.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // a command takes its arguments by value
fn notify(
    app: AppHandle,
    window: tauri::Window,
    title: String,
    body: String,
) -> Result<(), String> {
    if window.label() != "main" || window.is_focused().unwrap_or(false) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|why| why.to_string())
}

/// The page's window, brought in front of the operator.
fn bring_forward(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Sidecar(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![drive_authorization, notify])
        .setup(|app| {
            // A start that cannot drive says why and exits, as a refused
            // flag does, rather than panicking; a sidecar it started goes too.
            if let Err(why) = start(app) {
                if let Ok(mut held) = app.state::<Sidecar>().0.lock()
                    && let Some(held) = held.take()
                {
                    let _ = held.child.kill();
                }
                eprintln!("exercise: {why}");
                std::process::exit(1);
            }
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
            && let Some(held) = held.take()
        {
            let authorization = handle.state::<Credential>().0.clone();
            quit(held, authorization.as_deref());
        }
    });
}

/// The drive found, its credential held, the sidecar started when it is one,
/// and the page's window opened on it.
fn start(app: &mut tauri::App) -> Result<(), String> {
    let handle = app.handle().clone();
    // A SIGTERM, SIGINT or SIGHUP exits as a quit does, so the sidecar is
    // killed then too; only a SIGKILL leaves it running.
    let quit = handle.clone();
    ctrlc::set_handler(move || quit.exit(0)).map_err(|why| why.to_string())?;
    // A click on a notification (#1049) brings the page's window forward.
    let clicked = handle.clone();
    handle
        .notification()
        .on_action(move |_| bring_forward(&clicked))
        .map_err(|why| why.to_string())?;
    let settings = settings(&handle)?;
    let drive = setting(settings.as_ref())?;
    // The credential's text, `user:password`, and how the sidecar is given it.
    let (pair, auth) = match auth(settings.as_ref())? {
        None => (None, None),
        Some(Auth::File(path)) => (Some(read_auth_file(&path)?), Some(Auth::File(path))),
        Some(Auth::Keychain) => {
            let Drive::Profile(profile) = &drive else {
                return Err("the keychain's account is a profile, and a `drive` URL names none; use an auth file".to_owned());
            };
            (Some(keychain(profile)?), Some(Auth::Keychain))
        }
    };
    let credential = pair
        .as_deref()
        .map(|pair| {
            basic(pair)
                .ok_or_else(|| "the credential is not user:password, as serve reads it".to_owned())
        })
        .transpose()?;
    app.manage(Credential(credential));
    let drive = match drive {
        Drive::Remote(url) => url,
        Drive::Profile(profile) => {
            let given = match (&auth, &pair) {
                (Some(Auth::File(path)), _) => Some(Given::File(path)),
                (Some(Auth::Keychain), Some(pair)) => Some(Given::Stdin(pair)),
                _ => None,
            };
            let (url, held) = sidecar(&handle, &profile, given)?;
            if let Ok(mut slot) = handle.state::<Sidecar>().0.lock() {
                *slot = Some(held);
            }
            url
        }
    };
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page(&drive)?.into()))
        .title("exercise")
        .inner_size(1280.0, 860.0)
        // A link in model output opens in the system's browser when it is the
        // web's or mail's (#1050's audit, finding 19); no window ever opens here.
        .on_new_window(|url, _| {
            if openable(&url) {
                let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .build()
        .map_err(|why| why.to_string())?;
    Ok(())
}

/// settings.json in the app's config directory, when there is one.
fn settings(app: &AppHandle) -> Result<Option<(PathBuf, serde_json::Value)>, String> {
    let path = app
        .path()
        .app_config_dir()
        .map_err(|why| why.to_string())?
        .join("settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(why) => return Err(format!("{}: {why}", path.display())),
    };
    let value = serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(Some((path, value)))
}

/// A string setting from settings.json.
fn named(settings: Option<&(PathBuf, serde_json::Value)>, key: &str) -> Option<String> {
    settings?
        .1
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Where the drive is, as the module's doc orders the places it is named.
fn setting(settings: Option<&(PathBuf, serde_json::Value)>) -> Result<Drive, String> {
    if let Ok(url) = std::env::var("EXERCISE_DRIVE") {
        return remote(url);
    }
    if let Ok(profile) = std::env::var("EXERCISE_PROFILE") {
        return Ok(Drive::Profile(profile));
    }
    let Some((path, _)) = settings else {
        return Err("no settings.json in the app's config directory; name a `drive` URL or a serve `profile` there, or set EXERCISE_DRIVE or EXERCISE_PROFILE".to_owned());
    };
    match (named(settings, "drive"), named(settings, "profile")) {
        (Some(url), None) => remote(url),
        (None, Some(profile)) => Ok(Drive::Profile(profile)),
        _ => Err(format!(
            "{}: name exactly one of `drive` or `profile`",
            path.display()
        )),
    }
}

/// Where the drive's credential comes from, when one is named: the
/// keychain, by `EXERCISE_AUTH` or settings.json's `auth`, or an auth file,
/// by `EXERCISE_AUTH_FILE` or settings.json's `auth_file`; never both.
fn auth(settings: Option<&(PathBuf, serde_json::Value)>) -> Result<Option<Auth>, String> {
    let chosen = std::env::var("EXERCISE_AUTH")
        .ok()
        .or_else(|| named(settings, "auth"));
    let file = std::env::var("EXERCISE_AUTH_FILE")
        .ok()
        .or_else(|| named(settings, "auth_file"));
    chosen_auth(chosen.as_deref(), file)
}

/// [`auth`], from what was named.
fn chosen_auth(chosen: Option<&str>, file: Option<String>) -> Result<Option<Auth>, String> {
    match (chosen, file) {
        (None, None) => Ok(None),
        (None, Some(path)) => Ok(Some(Auth::File(path))),
        (Some("keychain"), None) => Ok(Some(Auth::Keychain)),
        (Some("keychain"), Some(_)) => {
            Err("the keychain and an auth file each name the credential: name one".to_owned())
        }
        (Some(other), _) => Err(format!("auth `{other}` is not `keychain`")),
    }
}

/// An auth file's text.
fn read_auth_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|why| format!("{path} cannot be read: {why}"))
}

/// The login keychain's generic password for `profile`, `user:password`.
/// macOS asks the operator, once, to let the app read it.
#[cfg(target_os = "macos")]
fn keychain(profile: &str) -> Result<String, String> {
    use security_framework::passwords::get_generic_password;
    let password = get_generic_password(KEYCHAIN_SERVICE, profile).map_err(|why| {
        format!("the keychain has no readable item {KEYCHAIN_SERVICE} for {profile}: {why}")
    })?;
    String::from_utf8(password)
        .map_err(|_| format!("the keychain's item {KEYCHAIN_SERVICE} for {profile} is not text"))
}

/// The keychain is macOS's.
#[cfg(not(target_os = "macos"))]
fn keychain(_profile: &str) -> Result<String, String> {
    Err(format!(
        "the keychain ({KEYCHAIN_SERVICE}) is read on macOS only; use an auth file"
    ))
}

/// `Basic <base64>` for an auth file's text, or none when serve would refuse it.
fn basic(written: &str) -> Option<String> {
    let pair = written
        .strip_suffix("\r\n")
        .or_else(|| written.strip_suffix('\n'))
        .unwrap_or(written);
    let usable = pair
        .split_once(':')
        .is_some_and(|(user, _)| !user.is_empty())
        && !pair.chars().any(char::is_control);
    usable.then(|| {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(pair)
        )
    })
}

/// A remote drive's URL, as the page takes it: https, or http on loopback.
/// The page sends the credential to it, so plain http leaves this machine
/// only refused; the CSP's `connect-src` admits the same.
fn remote(url: String) -> Result<Drive, String> {
    match Url::parse(&url) {
        Ok(parsed) if parsed.scheme() == "https" => Ok(Drive::Remote(url)),
        Ok(parsed)
            if parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")) =>
        {
            Ok(Drive::Remote(url))
        }
        _ => Err(format!(
            "`{url}` is no https URL, nor an http one on 127.0.0.1 or localhost"
        )),
    }
}

/// How the sidecar is given the credential.
#[derive(Clone, Copy)]
enum Given<'a> {
    /// `--auth-file`, by its path.
    File(&'a str),
    /// `--auth-stdin`, the text written as stdin's first line.
    Stdin(&'a str),
}

/// `serve --profile <profile>` as the sidecar, admitting the webview's
/// origin, with the shell's credential when it has one; its URL, from the
/// first line it prints, and the process.
fn sidecar(
    app: &AppHandle,
    profile: &str,
    given: Option<Given>,
) -> Result<(String, Held), String> {
    let mut args = vec!["serve", "--profile", profile, "--allow-origin", ORIGIN];
    match given {
        Some(Given::File(file)) => args.extend(["--auth-file", file]),
        Some(Given::Stdin(_)) => args.push("--auth-stdin"),
        None => {}
    }
    let (mut events, mut child) = app
        .shell()
        .sidecar("diet-drive")
        .map_err(|why| why.to_string())?
        .args(args)
        .spawn()
        .map_err(|why| why.to_string())?;
    if let Some(Given::Stdin(pair)) = given {
        // One line, as `--auth-stdin` reads it; every command serve runs is
        // given a null stdin, so none reads what is left on the pipe.
        child
            .write(format!("{pair}\n").as_bytes())
            .map_err(|why| format!("serve's stdin cannot be written: {why}"))?;
    }
    while let Some(event) = events.blocking_recv() {
        match event {
            CommandEvent::Stdout(line) => {
                let first: serde_json::Value = serde_json::from_slice(&line)
                    .map_err(|why| format!("serve's first line is no JSON: {why}"))?;
                let Some(listening) = first.get("listening").and_then(serde_json::Value::as_str)
                else {
                    return Err(format!("serve's first line names no address: {first}"));
                };
                // The page reaches serve only where its CSP admits (finding 23).
                if !csp_admits(listening) {
                    let _ = child.kill();
                    return Err(format!(
                        "serve listens on {listening}, which the page's CSP does not admit: \
                         give the profile --listen 127.0.0.1"
                    ));
                }
                let url = format!("http://{listening}/");
                let exited = Arc::new((Mutex::new(false), Condvar::new()));
                let told = Arc::clone(&exited);
                // What serve says after its first line goes to the shell's
                // own stderr, so the channel never fills; its exit is told to
                // a quit waiting on it.
                tauri::async_runtime::spawn(async move {
                    while let Some(event) = events.recv().await {
                        match event {
                            CommandEvent::Stderr(line) | CommandEvent::Stdout(line) => {
                                eprintln!("serve: {}", String::from_utf8_lossy(&line).trim_end());
                            }
                            CommandEvent::Terminated(ended) => {
                                eprintln!("serve: exited ({:?})", ended.code);
                                let (gone, changed) = &*told;
                                if let Ok(mut gone) = gone.lock() {
                                    *gone = true;
                                }
                                changed.notify_all();
                            }
                            _ => {}
                        }
                    }
                });
                let listening = listening.to_owned();
                return Ok((url, Held { child, listening, exited }));
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

/// The app is quitting (#1050's audit, finding 5): end the session as the
/// page would -- `end`, and with a turn in flight, a cancel of it first --
/// then wait for serve to write its record and exit, and kill it only if it
/// has not within [`QUIT_WAIT`]. No signal handler: #230's ruling.
fn quit(held: Held, authorization: Option<&str>) {
    let deadline = Instant::now() + QUIT_WAIT;
    while Instant::now() < deadline {
        match exchange(&held.listening, &command_request(&held.listening, authorization, r#"{"kind":"end"}"#)).as_deref().and_then(status_of) {
            Some(409) => {
                let state = exchange(&held.listening, &get_request(&held.listening, authorization, "/state"));
                if let Some(turn) = state.as_deref().and_then(|reply| reply.split_once("\r\n\r\n")).and_then(|(_, body)| running_turn(body)) {
                    let cancel = format!(r#"{{"kind":"cancel","turn":{turn}}}"#);
                    let _ = exchange(&held.listening, &command_request(&held.listening, authorization, &cancel));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            _ => break,
        }
    }
    let (gone, changed) = &*held.exited;
    let left = deadline.saturating_duration_since(Instant::now());
    let exited = gone
        .lock()
        .ok()
        .and_then(|guard| changed.wait_timeout_while(guard, left, |gone| !*gone).ok())
        .is_some_and(|(gone, _)| *gone);
    if !exited {
        let _ = held.child.kill();
    }
}

/// A `POST /commands` of `body`, with the credential when there is one, as
/// the page sends a command.
fn command_request(listening: &str, authorization: Option<&str>, body: &str) -> String {
    let credential = authorization.map(|value| format!("Authorization: {value}\r\n")).unwrap_or_default();
    format!(
        "POST /commands HTTP/1.1\r\nHost: {listening}\r\n{credential}Content-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A `GET` of `path`, with the credential when there is one.
fn get_request(listening: &str, authorization: Option<&str>, path: &str) -> String {
    let credential = authorization.map(|value| format!("Authorization: {value}\r\n")).unwrap_or_default();
    format!("GET {path} HTTP/1.1\r\nHost: {listening}\r\n{credential}Connection: close\r\n\r\n")
}

/// One request to serve on loopback, and its whole reply; none when serve
/// does not answer within a second or two.
fn exchange(listening: &str, request: &str) -> Option<String> {
    let mut stream = TcpStream::connect(listening).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(1))).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    Some(String::from_utf8_lossy(&reply).into_owned())
}

/// An HTTP reply's status code.
fn status_of(reply: &str) -> Option<u16> {
    reply.strip_prefix("HTTP/1.1 ")?.split(' ').next()?.parse().ok()
}

/// The turn `GET /state` says is running, or waits on an approval: what a
/// quit cancels so that `end` is taken.
fn running_turn(body: &str) -> Option<u64> {
    let state: serde_json::Value = serde_json::from_str(body).ok()?;
    let turn = state.get("turn")?;
    if matches!(turn.get("state")?.as_str()?, "running" | "awaiting-approval") {
        turn.get("number")?.as_u64()
    } else {
        None
    }
}

/// Whether a link the page opens goes to the system's browser: the web's
/// and mail's alone (#1050's audit, finding 19).
fn openable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto")
}

/// Whether the page's CSP (`connect-src`, `tauri.conf.json`) admits a drive
/// at `listening`, serve's `host:port`: loopback by name or by 127.0.0.1
/// (#1050's audit, finding 23).
fn csp_admits(listening: &str) -> bool {
    listening
        .rsplit_once(':')
        .is_some_and(|(host, port)| matches!(host, "127.0.0.1" | "localhost") && port.parse::<u16>().is_ok())
}

/// The bundle's page, told its drive by `?drive=<url>`.
fn page(drive: &str) -> Result<String, String> {
    let mut url = Url::parse("tauri://localhost/index.html").map_err(|why| why.to_string())?;
    url.query_pairs_mut().append_pair("drive", drive);
    Ok(format!("index.html?{}", url.query().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::{Auth, Drive, basic, chosen_auth, command_request, csp_admits, openable, remote, running_turn, status_of};

    /// #1050's audit, finding 5: the shell ends the session as the page
    /// would, a `POST /commands` with the credential, before it lets serve go.
    #[test]
    fn the_shell_ends_the_session_with_a_command_as_the_page_sends_one() {
        assert_eq!(
            command_request("127.0.0.1:7801", Some("Basic YTpi"), r#"{"kind":"end"}"#),
            "POST /commands HTTP/1.1\r\nHost: 127.0.0.1:7801\r\nAuthorization: Basic YTpi\r\n\
             Content-Type: application/json\r\nContent-Length: 14\r\nConnection: close\r\n\r\n\
             {\"kind\":\"end\"}"
        );
        assert!(!command_request("127.0.0.1:7801", None, "{}").contains("Authorization"));
        assert_eq!(status_of("HTTP/1.1 409 Conflict\r\n\r\n{}"), Some(409));
        assert_eq!(status_of("garbage"), None);
    }

    /// With a turn in flight `end` is refused (`in-flight`), so the shell
    /// cancels the turn `/state` names as running, then ends.
    #[test]
    fn a_running_turn_is_read_from_the_state() {
        assert_eq!(running_turn(r#"{"turn":{"number":3,"state":"running"}}"#), Some(3));
        assert_eq!(running_turn(r#"{"turn":{"number":3,"state":"awaiting-approval"}}"#), Some(3));
        assert_eq!(running_turn(r#"{"turn":{"number":3,"state":"settled","reason":"final"}}"#), None);
        assert_eq!(running_turn(r#"{"session":"idle"}"#), None);
    }

    /// #1050's audit, finding 19: a link in the page opens in the system's
    /// browser when it is the web's or mail's, and nothing else opens.
    #[test]
    fn a_link_opens_outside_only_for_the_web_and_mail() {
        for opened in [
            "https://docs.example/page",
            "http://localhost:5173/",
            "mailto:someone@example.com",
        ] {
            let url = tauri::Url::parse(opened).expect("a URL");
            assert!(openable(&url), "{opened}");
        }
        for kept in [
            "file:///etc/hosts",
            "javascript:alert(1)",
            "tauri://localhost/index.html",
            "data:text/html,hi",
            "ssh://example.com",
        ] {
            let url = tauri::Url::parse(kept).expect("a URL");
            assert!(!openable(&url), "{kept}");
        }
    }

    /// #1050's audit, finding 23: the page reaches the sidecar only where
    /// its CSP's `connect-src` admits, so an address it does not is refused
    /// at start, not met as an unreachable drive.
    #[test]
    fn the_sidecar_is_reached_only_where_the_csp_admits() {
        for admitted in ["127.0.0.1:7801", "localhost:7801"] {
            assert!(csp_admits(admitted), "{admitted}");
        }
        for refused in ["[::1]:7801", "0.0.0.0:7801", "drive.example:7801"] {
            assert!(!csp_admits(refused), "{refused}");
        }
    }

    #[test]
    fn the_credential_is_named_by_the_keychain_or_a_file_never_both() {
        assert!(matches!(chosen_auth(None, None), Ok(None)));
        assert!(matches!(
            chosen_auth(None, Some("auth".to_owned())),
            Ok(Some(Auth::File(path))) if path == "auth"
        ));
        assert!(matches!(
            chosen_auth(Some("keychain"), None),
            Ok(Some(Auth::Keychain))
        ));
        assert!(chosen_auth(Some("keychain"), Some("auth".to_owned())).is_err());
        assert!(chosen_auth(Some("file"), None).is_err());
    }

    #[test]
    fn an_auth_file_is_read_as_serve_reads_it() {
        // `user:pass`, base64: what a page's Basic header carries.
        let header = Some("Basic dXNlcjpwYXNz".to_owned());
        assert_eq!(basic("user:pass"), header);
        assert_eq!(basic("user:pass\n"), header);
        assert_eq!(basic("user:pass\r\n"), header);
        // Whitespace other than one trailing break is the credential's.
        assert_eq!(
            basic("user:pass \n"),
            Some("Basic dXNlcjpwYXNzIA==".to_owned())
        );
        for refused in ["", "\n", "nocolon", ":pass", "user:pa\nss", "user:pass\n\n"] {
            assert_eq!(basic(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_remote_drive_is_https_or_loopback_http() {
        for admitted in [
            "https://drive.example/",
            "http://127.0.0.1:7801/",
            "http://localhost:7801/",
        ] {
            assert!(
                matches!(remote(admitted.to_owned()), Ok(Drive::Remote(_))),
                "{admitted}"
            );
        }
        for refused in ["http://drive.example/", "file:///x", "drive"] {
            assert!(remote(refused.to_owned()).is_err(), "{refused}");
        }
    }
}
