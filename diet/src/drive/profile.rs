//! The per-user config (#717): endpoints and profiles, outside any
//! checkout, so `serve --profile floor` starts a session with no other
//! flags.
//!
//! The file is `$XDG_CONFIG_HOME/discipline/drive.toml`, or
//! `~/.config/discipline/drive.toml` with no `XDG_CONFIG_HOME`; `--config
//! FILE` names another. It is the user's own: nothing about it is in the
//! tree, which never names a host. It is read with the regimen's TOML
//! subset (`diet/formats/regimen/grammar.pest`), and holds two tables of
//! tables:
//!
//! ```toml
//! [endpoints.floor]
//! url = "http://HOST:PORT/v1/chat/completions"
//! key_file = "~/.config/discipline/floor.key"
//!
//! [profiles.floor]
//! endpoint = "floor"
//! model = "the-model"
//! head = "~/heads/floor.txt"
//! regimen = "~/regimens/floor.toml"
//! auth_file = "~/.config/discipline/drive.auth"
//! port = 7801
//! ```
//!
//! A profile's keys are `serve`'s flags without their dashes, `_` for `-`;
//! its `endpoint` names an `[endpoints]` table -- whose `url`, and whose
//! `key_file` unless the profile names one -- or is itself a URL. A key or
//! credential is named by its file's path, never written in: there is no
//! key that holds one. A path's leading `~` is the home directory. A flag on
//! the command line beats the profile's, and an `--allow-origin` given there
//! replaces the profile's list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::formats::regimen::{self, Value};

/// The config's file name, under its directory.
pub const FILE: &str = "drive.toml";

/// How a profile key becomes a flag's value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A path: a leading `~` expanded.
    Path,
    /// Text, as written.
    Text,
    /// A non-negative integer.
    Count,
    /// A list of text: the flag once for each.
    List,
}

/// The keys a profile may hold: `serve`'s flags.
const KEYS: &[(&str, Shape)] = &[
    ("endpoint", Shape::Text),
    ("model", Shape::Text),
    ("head", Shape::Path),
    ("key_file", Shape::Path),
    ("auth_file", Shape::Path),
    ("regimen", Shape::Path),
    ("log", Shape::Path),
    ("record", Shape::Path),
    ("listen", Shape::Text),
    ("port", Shape::Count),
    ("allow_origin", Shape::List),
    ("max_output_tokens", Shape::Count),
    ("worktree", Shape::Path),
    ("seat_endpoint", Shape::Text),
    ("seat_model", Shape::Text),
    ("seat_key_file", Shape::Path),
    ("ui", Shape::Path),
];

/// Where the config is by default: under `XDG_CONFIG_HOME`, or `HOME`'s
/// `.config`; `None` with neither.
#[must_use]
pub fn default_path(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let base = var("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("discipline").join(FILE))
}

/// `path` with a leading `~` or `~/` as `home`.
fn expanded(path: &str, home: Option<&Path>) -> String {
    match (path, home) {
        ("~", Some(home)) => home.to_string_lossy().into_owned(),
        (path, Some(home)) if path.starts_with("~/") => {
            home.join(&path[2..]).to_string_lossy().into_owned()
        }
        (path, _) => path.to_owned(),
    }
}

/// The `[NAME]` table of `table` in `document`, or why there is none.
fn table_of<'a>(
    document: &'a regimen::Regimen,
    table: &str,
    name: &str,
) -> Result<Option<&'a BTreeMap<String, Value>>, String> {
    match document.get(table) {
        None => Ok(None),
        Some(Value::Table(entries)) => match entries.get(name) {
            None => Ok(None),
            Some(Value::Table(entry)) => Ok(Some(entry)),
            Some(_) => Err(format!("`{table}.{name}` is not a table")),
        },
        Some(_) => Err(format!("`{table}` is not a table of tables")),
    }
}

/// The flags profile `name` in config `text` stands for, in `KEYS`' order,
/// paths under `home`.
///
/// # Errors
///
/// When the config does not read, names no such profile, or holds a key
/// or value a profile cannot.
pub fn flags(text: &str, name: &str, home: Option<&Path>) -> Result<Vec<String>, String> {
    let document = regimen::parse(text).map_err(|why| format!("it does not read: {why:?}"))?;
    for (key, _) in &document {
        if key != "endpoints" && key != "profiles" {
            return Err(format!(
                "`{key}` is neither `endpoints` nor `profiles`, the two tables it holds"
            ));
        }
    }
    let Some(profile) = table_of(&document, "profiles", name)? else {
        return Err(format!("it names no profile `{name}`"));
    };
    let mut values: BTreeMap<&str, Value> = BTreeMap::new();
    for (key, value) in profile {
        let Some((known, _)) = KEYS.iter().find(|(known, _)| known == key) else {
            return Err(format!(
                "profile `{name}`'s `{key}` is not one of serve's flags; a key or credential is \
                 named by its file (`key_file`, `auth_file`), never written in"
            ));
        };
        values.insert(known, value.clone());
    }
    // An endpoint named by its table: its URL, and its key file unless the
    // profile names its own.
    if let Some(Value::String(endpoint)) = values.get("endpoint").cloned()
        && let Some(table) = table_of(&document, "endpoints", &endpoint)?
    {
        for (key, value) in table {
            match (key.as_str(), value) {
                ("url", Value::String(url)) => {
                    values.insert("endpoint", Value::String(url.clone()));
                }
                ("key_file", Value::String(_)) => {
                    values.entry("key_file").or_insert_with(|| value.clone());
                }
                _ => {
                    return Err(format!(
                        "endpoint `{endpoint}`'s `{key}` is neither its `url` nor its `key_file`, \
                         each a string"
                    ));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (key, shape) in KEYS {
        let Some(value) = values.get(key) else {
            continue;
        };
        let flag = format!("--{}", key.replace('_', "-"));
        let wrong = |want: &str| format!("profile `{name}`'s `{key}` must be {want}");
        match (shape, value) {
            (Shape::Path, Value::String(path)) => out.extend([flag, expanded(path, home)]),
            (Shape::Text, Value::String(text)) => out.extend([flag, text.clone()]),
            (Shape::Count, Value::Integer(n)) if *n >= 0 => out.extend([flag, n.to_string()]),
            (Shape::List, Value::Array(items)) => {
                for item in items {
                    let Value::String(text) = item else {
                        return Err(wrong("a list of strings"));
                    };
                    out.extend([flag.clone(), text.clone()]);
                }
            }
            (Shape::Count, _) => return Err(wrong("a non-negative integer")),
            (Shape::List, _) => return Err(wrong("a list of strings")),
            (Shape::Path | Shape::Text, _) => return Err(wrong("a string")),
        }
    }
    Ok(out)
}

/// `args` -- a subcommand's flags, in pairs -- with `--profile NAME` and
/// `--config FILE` taken out and the profile's flags put first, less any
/// the command line gives itself; `args` as they were with no `--profile`.
/// `config` is where the config is when `--config` names none.
///
/// # Errors
///
/// When the config cannot be read or its profile is not one, naming the
/// file.
pub fn with_profile(
    args: &[String],
    config: Option<PathBuf>,
    home: Option<&Path>,
) -> Result<Vec<String>, String> {
    let mut profile = None;
    let mut named = None;
    let mut rest = Vec::new();
    let mut pairs = args.chunks(2);
    for pair in pairs.by_ref() {
        match pair {
            [flag, value] if flag == "--profile" => profile = Some(value.clone()),
            [flag, value] if flag == "--config" => named = Some(PathBuf::from(value)),
            pair => rest.extend_from_slice(pair),
        }
    }
    let Some(profile) = profile else {
        return Ok(args.to_vec());
    };
    let Some(path) = named.or(config) else {
        return Err(
            "no config: neither XDG_CONFIG_HOME nor HOME is set, and --config names none"
                .to_owned(),
        );
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|why| format!("the config {} cannot be read: {why}", path.display()))?;
    let flags = flags(&text, &profile, home)
        .map_err(|why| format!("the config {}: {why}", path.display()))?;
    let given: Vec<&String> = rest.iter().step_by(2).collect();
    let mut out = Vec::new();
    for pair in flags.chunks(2) {
        if !given.contains(&&pair[0]) {
            out.extend_from_slice(pair);
        }
    }
    out.extend(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "[endpoints.floor]\n\
        url = \"http://floor.test:5000/v1/chat/completions\"\n\
        key_file = \"~/.config/discipline/floor.key\"\n\
        \n\
        [profiles.floor]\n\
        endpoint = \"floor\"\n\
        model = \"m\"\n\
        head = \"~/heads/h.txt\"\n\
        port = 7801\n\
        allow_origin = [\"http://localhost:5173\", \"http://a.test\"]\n\
        \n\
        [profiles.direct]\n\
        endpoint = \"http://127.0.0.1:7901\"\n\
        model = \"m\"\n\
        head = \"/h.txt\"\n";

    fn strings(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn a_profile_is_serves_flags_its_endpoint_resolved_and_its_paths_home() {
        let home = Path::new("/the-home");
        assert_eq!(
            flags(CONFIG, "floor", Some(home)).expect("flags"),
            strings(&[
                "--endpoint",
                "http://floor.test:5000/v1/chat/completions",
                "--model",
                "m",
                "--head",
                "/the-home/heads/h.txt",
                "--key-file",
                "/the-home/.config/discipline/floor.key",
                "--port",
                "7801",
                "--allow-origin",
                "http://localhost:5173",
                "--allow-origin",
                "http://a.test",
            ])
        );
        assert_eq!(
            flags(CONFIG, "direct", Some(home)).expect("flags"),
            strings(&[
                "--endpoint",
                "http://127.0.0.1:7901",
                "--model",
                "m",
                "--head",
                "/h.txt"
            ])
        );
    }

    #[test]
    fn a_key_written_in_or_an_unknown_profile_is_refused() {
        let inlined = "[profiles.p]\nendpoint = \"http://x:1\"\nkey = \"sk-123\"\n";
        assert!(
            flags(inlined, "p", None)
                .expect_err("refused")
                .contains("never written in")
        );
        assert_eq!(
            flags(CONFIG, "nope", None).expect_err("refused"),
            "it names no profile `nope`"
        );
        assert!(flags("[other]\nx = 1\n", "p", None).is_err());
        assert!(flags("[profiles.p]\nport = \"7801\"\n", "p", None).is_err());
    }

    #[test]
    fn the_command_line_beats_the_profile_and_the_default_path_is_xdgs() {
        let dir = std::env::temp_dir().join(format!("diet-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a dir");
        let config = dir.join(FILE);
        std::fs::write(&config, CONFIG).expect("written");
        let args = strings(&["--profile", "floor", "--port", "9000", "--log", "/l"]);
        let out = with_profile(&args, Some(config.clone()), None).expect("expanded");
        assert!(out.windows(2).any(|pair| pair == ["--port", "9000"]));
        assert!(!out.windows(2).any(|pair| pair == ["--port", "7801"]));
        assert!(out.windows(2).any(|pair| pair == ["--model", "m"]));
        assert!(!out.contains(&"--profile".to_owned()));
        let named = strings(&["--config", &config.to_string_lossy(), "--profile", "direct"]);
        assert_eq!(
            with_profile(&named, None, None).expect("expanded")[..2],
            strings(&["--endpoint", "http://127.0.0.1:7901"])[..]
        );
        let plain = strings(&["--model", "m"]);
        assert_eq!(with_profile(&plain, None, None).expect("as is"), plain);
        assert!(
            with_profile(
                &strings(&["--profile", "p"]),
                Some(dir.join("none.toml")),
                None
            )
            .expect_err("no file")
            .contains("cannot be read")
        );
        let var = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some("/x".into()),
            "HOME" => Some("/h".into()),
            _ => None,
        };
        assert_eq!(
            default_path(var),
            Some(PathBuf::from("/x/discipline/drive.toml"))
        );
        let home_only = |name: &str| (name == "HOME").then(|| "/h".into());
        assert_eq!(
            default_path(home_only),
            Some(PathBuf::from("/h/.config/discipline/drive.toml"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
