//! A scenario's services (#942): what a run needs beside serve, outside the
//! model's sandbox -- a dev server, a browser daemon -- started before serve
//! and stopped after it, each run its own.
//!
//! A service is `run`, a command that stays in the foreground, whose process
//! group the runner holds and stops; or `start` and `stop`, for a daemon that
//! detaches, whose `start` writes the PIDs it leaves to `$DIET_SERVICE_DIR/pids`
//! and whose `stop` the runner runs, killing any of those PIDs still alive
//! after it. Never a pattern. Each waits to be `ready`: a port that accepts
//! a connection, or a command that exits 0. A service may hand values on in
//! `$DIET_SERVICE_DIR/provides.json`, an object of strings.
//!
//! What a run is told, and where: each named port, reserved for the run, as
//! `DIET_PORT_<KEY>`; each provided value as `DIET_SERVICE_<NAME>_<KEY>`,
//! upper-cased with `-` as `_`, in every later service's environment, every
//! check's, and serve's (which still passes on to a command only what the
//! regimen's `env_passthrough` names); and in an ask's text, and a grade's
//! feedback, as `{port.KEY}` and `{service.NAME.KEY}`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::formats::record::json::Value;

/// How long a service may take to be ready unless it names its own.
pub const READY_TIMEOUT_S: u64 = 60;

/// How long a daemon's `start` or `stop` may run.
pub const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// One service, as the scenario declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Its name: its directory, and its values' prefix.
    pub name: String,
    /// How it runs and is stopped.
    pub runs: Runs,
    /// What says it is ready.
    pub ready: Ready,
    /// How long it may take to be ready.
    pub ready_timeout_s: u64,
}

/// How a service runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runs {
    /// A command that stays in the foreground: its process group is stopped.
    Foreground(String),
    /// A daemon that detaches: its `start`, then, after the run, its `stop`.
    Daemon {
        /// What starts it, writing the PIDs it leaves to `pids`.
        start: String,
        /// What stops it.
        stop: String,
    },
}

/// What says a service is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    /// A named port accepts a connection on loopback.
    Tcp(String),
    /// A command that exits 0, polled.
    Command(String),
}

/// A service's name, or a port's or a value's key: lowercase letters,
/// digits, `-` and `_`, starting with a letter.
#[must_use]
pub fn is_a_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A name as an environment variable's part: upper-cased, `-` as `_`.
fn upper(name: &str) -> String {
    name.to_ascii_uppercase().replace('-', "_")
}

/// The services a header's `services` declares, its ports named in
/// `ports`.
///
/// # Errors
///
/// When a service is not an object of the keys it takes, names itself
/// badly or twice, takes both `run` and `start`, or waits on a port
/// `ports` does not name.
pub fn services_of(declared: Vec<Value>, ports: &[String]) -> Result<Vec<Service>, String> {
    let mut services: Vec<Service> = Vec::new();
    for (at, service) in declared.into_iter().enumerate() {
        let Value::Object(mut fields) = service else {
            return Err(format!("`services[{at}]` is not an object"));
        };
        let mut text = |key: &str| match fields.remove(key) {
            None => Ok(None),
            Some(Value::String(text)) if !text.is_empty() => Ok(Some(text)),
            Some(_) => Err(format!("`services[{at}]`'s `{key}` is not text")),
        };
        let name = text("name")?.ok_or_else(|| format!("`services[{at}]` names no `name`"))?;
        if !is_a_name(&name) || services.iter().any(|service| service.name == name) {
            return Err(format!(
                "`services[{at}]`'s name `{name}` is not a name (lowercase letters, digits, `-` \
                 and `_`), or is another service's"
            ));
        }
        let runs = match (text("run")?, text("start")?, text("stop")?) {
            (Some(run), None, None) => Runs::Foreground(run),
            (None, Some(start), Some(stop)) => Runs::Daemon { start, stop },
            _ => {
                return Err(format!(
                    "service `{name}` takes `run`, or `start` and `stop`: one or the other"
                ));
            }
        };
        let ready = match fields.remove("ready") {
            Some(Value::Object(mut ready)) => {
                let tcp = ready.remove("tcp");
                let command = ready.remove("command");
                if !ready.is_empty() {
                    return Err(format!(
                        "service `{name}`'s `ready` takes `tcp` or `command`"
                    ));
                }
                match (tcp, command) {
                    (Some(Value::String(port)), None) if ports.contains(&port) => Ready::Tcp(port),
                    (Some(Value::String(port)), None) => {
                        return Err(format!(
                            "service `{name}` waits on port `{port}`, which `ports` does not name"
                        ));
                    }
                    (None, Some(Value::String(command))) if !command.is_empty() => {
                        Ready::Command(command)
                    }
                    _ => {
                        return Err(format!(
                            "service `{name}`'s `ready` is `{{\"tcp\": PORT}}` or `{{\"command\": TEXT}}`"
                        ));
                    }
                }
            }
            _ => {
                return Err(format!(
                    "service `{name}` says nothing of when it is `ready`"
                ));
            }
        };
        let ready_timeout_s = match fields.remove("ready_timeout_s") {
            None => READY_TIMEOUT_S,
            Some(Value::Integer(n)) if n > 0 => n.unsigned_abs(),
            Some(_) => {
                return Err(format!(
                    "service `{name}`'s `ready_timeout_s` is not a count above 0"
                ));
            }
        };
        if let Some(key) = fields.keys().next() {
            return Err(format!("service `{name}` does not take `{key}`"));
        }
        services.push(Service {
            name,
            runs,
            ready,
            ready_timeout_s,
        });
    }
    Ok(services)
}

/// Every `{port.KEY}` and `{service.NAME.KEY}` in `text`: each as its
/// place's words, `["port", KEY]` or `["service", NAME, KEY]`. Other braces
/// are text.
#[must_use]
pub fn placeholders(text: &str) -> Vec<Vec<&str>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let words: Vec<&str> = after[..close].split('.').collect();
        let named = match words.as_slice() {
            ["port", key] => is_a_name(key),
            ["service", name, key] => is_a_name(name) && is_a_name(key),
            _ => false,
        };
        if named {
            found.push(words);
            rest = &after[close + 1..];
        } else {
            rest = after;
        }
    }
    found
}

/// What a run of a scenario's services holds while it runs.
#[derive(Debug)]
pub struct Running {
    /// Where each service keeps its output, its PIDs and what it provides.
    dir: PathBuf,
    /// Each port's number.
    ports: BTreeMap<String, u16>,
    /// Each service's provided values.
    provided: BTreeMap<String, BTreeMap<String, String>>,
    /// The services started, in order, with what a stop needs.
    started: Vec<Started>,
    /// What each did: ready, and stopped.
    said: BTreeMap<String, serde_json::Value>,
}

/// A service started.
#[derive(Debug)]
struct Started {
    service: Service,
    /// A foreground service's process.
    child: Option<std::process::Child>,
    /// Its directory.
    dir: PathBuf,
}

/// A port on loopback no one holds now.
fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|bound| std::net::TcpListener::local_addr(&bound))
        .map(|address| address.port())
        .map_err(|why| format!("no port can be reserved: {why}"))
}

/// Whether `pid` is alive.
fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", "--", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Send `signal` to `pid`, or, `group`, to its process group.
fn signal(signal: &str, pid: u32, group: bool) {
    let target = if group {
        format!("-{pid}")
    } else {
        pid.to_string()
    };
    let _ = std::process::Command::new("/bin/kill")
        .args([signal, "--", &target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

impl Running {
    /// Start `services`, in order, in `cwd` with `env` beside each one's
    /// own, keeping their files under `dir`. `worktree` is named to them as
    /// `DIET_WORKTREE`.
    ///
    /// # Errors
    ///
    /// When a port cannot be reserved, a service cannot start, or one is
    /// not ready in time: what each service did, those started stopped
    /// first, and why.
    pub fn start(
        services: &[Service],
        ports: &[String],
        (dir, cwd, worktree): (&Path, &Path, Option<&Path>),
        env: &[(String, String)],
    ) -> Result<Self, (serde_json::Value, String)> {
        let mut running = Self {
            dir: dir.to_path_buf(),
            ports: BTreeMap::new(),
            provided: BTreeMap::new(),
            started: Vec::new(),
            said: BTreeMap::new(),
        };
        for port in ports {
            match free_port() {
                Ok(number) => {
                    running.ports.insert(port.clone(), number);
                }
                Err(why) => return Err((running.said(), why)),
            }
        }
        for service in services {
            if let Err(why) = running.started_one(service, cwd, worktree, env) {
                running.stop(cwd, env);
                return Err((running.said(), format!("service `{}`: {why}", service.name)));
            }
        }
        Ok(running)
    }

    /// The environment a service, a check or serve is given: the ports and
    /// every value provided so far.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = self
            .ports
            .iter()
            .map(|(key, port)| (format!("DIET_PORT_{}", upper(key)), port.to_string()))
            .collect();
        for (name, values) in &self.provided {
            for (key, value) in values {
                env.push((
                    format!("DIET_SERVICE_{}_{}", upper(name), upper(key)),
                    value.clone(),
                ));
            }
        }
        env
    }

    /// The value at a placeholder's place, when there is one.
    #[must_use]
    pub fn value(&self, place: &[&str]) -> Option<String> {
        match place {
            ["port", key] => self.ports.get(*key).map(u16::to_string),
            ["service", name, key] => self.provided.get(*name)?.get(*key).cloned(),
            _ => None,
        }
    }

    /// `text` with every placeholder filled.
    ///
    /// # Errors
    ///
    /// When one names a port or a value no service gave.
    pub fn filled(&self, text: &str) -> Result<String, String> {
        let mut out = text.to_owned();
        for place in placeholders(text) {
            let value = self.value(&place).ok_or_else(|| {
                format!(
                    "`{{{}}}` names a port or a value no service gave",
                    place.join(".")
                )
            })?;
            out = out.replace(&format!("{{{}}}", place.join(".")), &value);
        }
        Ok(out)
    }

    /// What each service did, by name.
    #[must_use]
    pub fn said(&self) -> serde_json::Value {
        serde_json::json!(self.said)
    }

    fn started_one(
        &mut self,
        service: &Service,
        cwd: &Path,
        worktree: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<(), String> {
        let dir = self.dir.join(&service.name);
        std::fs::create_dir_all(&dir)
            .map_err(|why| format!("{} cannot be made: {why}", dir.display()))?;
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let mut told: Vec<(String, String)> = env.to_vec();
        told.extend(self.env());
        told.push(("DIET_SERVICE_DIR".to_owned(), dir.display().to_string()));
        if let Some(worktree) = worktree {
            told.push(("DIET_WORKTREE".to_owned(), worktree.display().to_string()));
        }
        let began = std::time::Instant::now();
        let child = match &service.runs {
            Runs::Foreground(run) => Some(spawned(run, cwd, &told, &dir)?),
            Runs::Daemon { start, .. } => {
                let status = ran(start, cwd, &told, &dir, COMMAND_TIMEOUT)?;
                if status != Some(0) {
                    self.started.push(Started {
                        service: service.clone(),
                        child: None,
                        dir: dir.clone(),
                    });
                    return Err(format!("`start` exited {status:?}"));
                }
                None
            }
        };
        self.started.push(Started {
            service: service.clone(),
            child,
            dir: dir.clone(),
        });
        let give_up = began + std::time::Duration::from_secs(service.ready_timeout_s);
        loop {
            if let Some(Some(child)) = self
                .started
                .last_mut()
                .map(|started| started.child.as_mut())
                && let Ok(Some(status)) = child.try_wait()
            {
                return Err(format!("it exited before it was ready: {status}"));
            }
            let ready = match &service.ready {
                Ready::Tcp(port) => self.ports.get(port).is_some_and(|port| {
                    std::net::TcpStream::connect_timeout(
                        &std::net::SocketAddr::from(([127, 0, 0, 1], *port)),
                        std::time::Duration::from_millis(500),
                    )
                    .is_ok()
                }),
                Ready::Command(command) => {
                    ran(
                        command,
                        cwd,
                        &told,
                        &dir,
                        std::time::Duration::from_secs(30),
                    )? == Some(0)
                }
            };
            if ready {
                break;
            }
            if std::time::Instant::now() >= give_up {
                return Err(format!("not ready in {} s", service.ready_timeout_s));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let provides = dir.join("provides.json");
        if provides.exists() {
            let text = std::fs::read_to_string(&provides)
                .map_err(|why| format!("{} cannot be read: {why}", provides.display()))?;
            let read: serde_json::Value = serde_json::from_str(&text)
                .map_err(|why| format!("{}: {why}", provides.display()))?;
            let Some(read) = read.as_object() else {
                return Err(format!("{} is not an object", provides.display()));
            };
            let mut values = BTreeMap::new();
            for (key, value) in read {
                match value.as_str() {
                    Some(value) if is_a_name(key) => {
                        values.insert(key.clone(), value.to_owned());
                    }
                    _ => {
                        return Err(format!(
                            "{}: `{key}` is not a name holding text",
                            provides.display()
                        ));
                    }
                }
            }
            self.provided.insert(service.name.clone(), values);
        }
        self.said.insert(
            service.name.clone(),
            serde_json::json!({
                "ready_ms": u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
            }),
        );
        Ok(())
    }

    /// Stop every service started, in reverse order: a foreground one by
    /// its process group, a daemon by its `stop` and then any of its PIDs
    /// still alive. Says what each stop did.
    pub fn stop(&mut self, cwd: &Path, env: &[(String, String)]) {
        let told_base = {
            let mut told = env.to_vec();
            told.extend(self.env());
            told
        };
        while let Some(mut started) = self.started.pop() {
            let mut told = told_base.clone();
            told.push((
                "DIET_SERVICE_DIR".to_owned(),
                started.dir.display().to_string(),
            ));
            let mut stopped = serde_json::Map::new();
            if let Some(child) = started.child.as_mut() {
                let killed = stop_child(child);
                stopped.insert("killed".to_owned(), serde_json::Value::Bool(killed));
            }
            if let Runs::Daemon { stop, .. } = &started.service.runs {
                let status = ran(stop, cwd, &told, &started.dir, COMMAND_TIMEOUT);
                stopped.insert(
                    "stop_exit".to_owned(),
                    match status {
                        Ok(Some(code)) => serde_json::json!(code),
                        Ok(None) => serde_json::Value::String("killed".to_owned()),
                        Err(why) => serde_json::Value::String(why),
                    },
                );
                let left: Vec<u32> = std::fs::read_to_string(started.dir.join("pids"))
                    .unwrap_or_default()
                    .split_whitespace()
                    .filter_map(|pid| pid.parse().ok())
                    .filter(|pid: &u32| *pid > 1 && alive(*pid))
                    .collect();
                for pid in &left {
                    signal("-TERM", *pid, false);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                for pid in left.iter().filter(|pid| alive(**pid)) {
                    signal("-KILL", *pid, false);
                }
                stopped.insert("pids_left_after_stop".to_owned(), serde_json::json!(left));
            }
            let entry = self
                .said
                .entry(started.service.name.clone())
                .or_insert_with(|| serde_json::json!({}));
            entry["stopped"] = serde_json::Value::Object(stopped);
        }
    }
}

impl Drop for Running {
    /// A run that ends without its stop still stops what it started.
    fn drop(&mut self) {
        if !self.started.is_empty() {
            let cwd = self.dir.clone();
            self.stop(&cwd, &[]);
        }
    }
}

/// Stop a foreground service's process group: TERM, then KILL after 10 s.
/// Whether it needed the KILL.
fn stop_child(child: &mut std::process::Child) -> bool {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return false;
    }
    signal("-TERM", child.id(), true);
    let give_up = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < give_up {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    signal("-KILL", child.id(), true);
    let _ = child.kill();
    let _ = child.wait();
    true
}

/// `command` under `bash -c` in `cwd`, leading its own process group, its
/// output appended to `dir/out.log`.
fn command(
    text: &str,
    cwd: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Result<std::process::Command, String> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("out.log"))
        .map_err(|why| format!("{} cannot be opened: {why}", dir.join("out.log").display()))?;
    let err = log
        .try_clone()
        .map_err(|why| format!("its log cannot be shared: {why}"))?;
    let mut command = std::process::Command::new("bash");
    command
        .arg("-c")
        .arg(text)
        .current_dir(cwd)
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    Ok(command)
}

/// A foreground service started.
fn spawned(
    text: &str,
    cwd: &Path,
    env: &[(String, String)],
    dir: &Path,
) -> Result<std::process::Child, String> {
    command(text, cwd, env, dir)?
        .spawn()
        .map_err(|why| format!("it cannot start: {why}"))
}

/// A command run to its end, or killed with its group at `timeout`: its
/// exit status, or `None` when it was killed.
fn ran(
    text: &str,
    cwd: &Path,
    env: &[(String, String)],
    dir: &Path,
    timeout: std::time::Duration,
) -> Result<Option<i32>, String> {
    let mut child = spawned(text, cwd, env, dir)?;
    let give_up = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
            Ok(None) if std::time::Instant::now() < give_up => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                signal("-KILL", child.id(), true);
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #942: a placeholder names a port or a service's value; any other
    /// braces are text.
    #[test]
    fn placeholders_are_ports_and_values_and_nothing_else() {
        assert_eq!(
            placeholders(
                "open http://127.0.0.1:{port.model_vite}/{id}/ via {service.browser.socket} {x}"
            ),
            [
                vec!["port", "model_vite"],
                vec!["service", "browser", "socket"]
            ]
        );
        assert!(placeholders("fn main() { let x = {a: 1}; }").is_empty());
    }

    /// #942: what `services` is refused for, each refusal naming it.
    #[test]
    fn a_service_is_refused_naming_what_is_wrong() {
        let read = |text: &str| -> Result<Vec<Service>, String> {
            let mut fields = crate::formats::record::json::objects(text)
                .expect("JSON")
                .remove(0);
            let Some(Value::Array(declared)) = fields.remove("s") else {
                panic!("an array");
            };
            services_of(declared, &["web".to_owned()])
        };
        let ok = read(r#"{"s":[{"name":"web","run":"x","ready":{"tcp":"web"}},{"name":"b","start":"s","stop":"t","ready":{"command":"c"},"ready_timeout_s":5}]}"#)
            .expect("they read");
        assert_eq!(ok[0].runs, Runs::Foreground("x".to_owned()));
        assert_eq!(ok[1].ready, Ready::Command("c".to_owned()));
        assert_eq!(ok[1].ready_timeout_s, 5);
        for (text, says) in [
            (
                r#"{"s":[{"run":"x","ready":{"tcp":"web"}}]}"#,
                "names no `name`",
            ),
            (
                r#"{"s":[{"name":"Web","run":"x","ready":{"tcp":"web"}}]}"#,
                "is not a name",
            ),
            (
                r#"{"s":[{"name":"a","run":"x","ready":{"tcp":"web"}},{"name":"a","run":"y","ready":{"tcp":"web"}}]}"#,
                "another service's",
            ),
            (
                r#"{"s":[{"name":"a","run":"x","start":"s","stop":"t","ready":{"tcp":"web"}}]}"#,
                "one or the other",
            ),
            (
                r#"{"s":[{"name":"a","start":"s","ready":{"tcp":"web"}}]}"#,
                "one or the other",
            ),
            (
                r#"{"s":[{"name":"a","run":"x","ready":{"tcp":"db"}}]}"#,
                "`ports` does not name",
            ),
            (r#"{"s":[{"name":"a","run":"x"}]}"#, "`ready`"),
            (
                r#"{"s":[{"name":"a","run":"x","ready":{"tcp":"web"},"user":"root"}]}"#,
                "does not take `user`",
            ),
        ] {
            let refused = read(text).expect_err(text);
            assert!(refused.contains(says), "{text}: {refused}");
        }
    }
}
