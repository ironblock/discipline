//! Background commands (#614): a `bash` call started with `is_background`
//! returns at once with a job's id, and the command keeps running under the
//! session's confinement, its output written to a file; `task_stop` stops
//! it; the operator can move a running call to the background.
//!
//! The shape is Qwen Code's, whole (Dispatch's ruling, #614): the only
//! compared harness whose shell runs in the background (`OpenCode` 2's
//! background jobs are its subagents', and Pi has none). Its words are
//! adapted as #557's were: no rule this harness does not enforce, and no
//! other harness's tool or dialog names (`tools/shell.ts`,
//! `tools/task-stop.ts`, `services/backgroundShellRegistry.ts`).

use std::path::{Path, PathBuf};

use crate::client::shape::ToolDefinition;

/// The stop tool's name, Qwen Code's.
pub const TASK_STOP: &str = "task_stop";

/// `bash`'s background argument, Qwen Code's name.
pub const IS_BACKGROUND: &str = "is_background";

/// `is_background`'s description, Qwen Code's (`tools/shell.ts:5759-5763`).
pub const IS_BACKGROUND_DESCRIPTION: &str = "Optional: Whether to run the command in background. \
     If not specified, defaults to false (foreground execution). Explicitly set to true for \
     long-running processes like development servers, watchers, or daemons that should continue \
     running without blocking further commands.";

/// `task_stop`: Qwen Code's (`tools/task-stop.ts:243-252`), its agents'
/// clause dropped -- only a shell runs in the background here.
#[must_use]
pub fn task_stop_tool() -> ToolDefinition {
    super::standard::definition(
        TASK_STOP,
        "Stop a background task by its ID. A running background shell is cancelled.",
        &[(
            "task_id",
            super::standard::property(
                "string",
                "The ID of the background task to stop (from the launch response or notification).",
            ),
        )],
        &["task_id"],
    )
}

/// How a job stands: Qwen Code's status words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Still running.
    Running,
    /// Exited 0.
    Completed,
    /// Exited otherwise.
    Failed,
    /// A stop ended it.
    Cancelled,
}

impl Status {
    /// Its word.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One background job.
#[derive(Debug, Clone)]
pub struct Job {
    /// `bg_` and eight hex digits.
    pub id: String,
    /// The command as the model wrote it.
    pub command: String,
    /// Its working directory, as the session names it.
    pub cwd: String,
    /// Its leader's pid, which leads its process group.
    pub pid: u32,
    /// Where its output goes.
    pub output: PathBuf,
    /// Where its status is kept.
    pub status_file: PathBuf,
    /// How it stands.
    pub status: Status,
    /// Its exit status, once it exited.
    pub exit: Option<i32>,
    /// Whether a stop was asked of it.
    pub stopping: bool,
}

impl Job {
    /// The status file's text: a JSON object, as Qwen Code's sidecar is.
    #[must_use]
    pub fn status_text(&self) -> String {
        let exit = self
            .exit
            .map_or_else(|| "null".to_owned(), |code| code.to_string());
        format!(
            "{{\"id\":{},\"status\":\"{}\",\"pid\":{},\"exit_code\":{exit}}}\n",
            serde_json::Value::from(self.id.as_str()),
            self.status.word(),
            self.pid
        )
    }

    /// Write the status file.
    pub fn write_status(&self) {
        let _ = std::fs::write(&self.status_file, self.status_text());
    }
}

/// A fresh job id: `bg_` and eight hex digits of a digest of the moment
/// and a counter, so two jobs in one session never share one.
#[must_use]
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let seed = format!(
        "{nanos}:{}:{}",
        COUNT.fetch_add(1, Ordering::SeqCst),
        std::process::id()
    );
    format!("bg_{}", &crate::digest::sha256_hex(seed.as_bytes())[..8])
}

/// The files job `id` writes in `dir`: its output, and its status beside it.
#[must_use]
pub fn files_of(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("shell-{id}.output")),
        dir.join(format!("shell-{id}.status")),
    )
}

/// `command` with one bare trailing `&` taken off, as Qwen Code takes it
/// off a background command (`tools/shell.ts:139-151`): never `&&`, never
/// an escaped `\&`.
#[must_use]
pub fn without_trailing_amp(command: &str) -> String {
    let trimmed = command.trim_end();
    if !trimmed.ends_with('&') || trimmed.ends_with("&&") || trimmed.ends_with("\\&") {
        return command.to_owned();
    }
    trimmed[..trimmed.len() - 1].trim_end().to_owned()
}

/// Qwen Code's liveness guidance (`tools/shell.ts:126-136`), its tool name
/// made a verb.
fn status_guidance(status_file: &Path) -> String {
    format!(
        "status file: {}\nTo check whether this process is still running, read the status file \
         (status: running|completed|failed|cancelled). Do NOT infer liveness from the output \
         file: programs often block-buffer stdout when not attached to a TTY, so an empty output \
         file is normal while the process is alive (for live output, re-run with e.g. `python -u` \
         or `stdbuf -oL`).",
        status_file.display()
    )
}

/// What a background start answers (`tools/shell.ts:4318-4327`), its
/// dialog line dropped.
#[must_use]
pub fn started(job: &Job) -> String {
    format!(
        "Background shell started.\nid: {}\npid: {}\noutput file: {}\n{}\nRead the output file \
         directly to view the captured output.",
        job.id,
        job.pid,
        job.output.display(),
        status_guidance(&job.status_file)
    )
}

/// What a promoted call answers (`tools/shell.ts:4064-4089`), its dialog
/// named nowhere.
#[must_use]
pub fn promoted(job: &Job) -> String {
    format!(
        "Foreground command \"{}\" promoted to background as {}.\nStatus: running. PID: {}.\n\
         Output snapshot at promote time saved to: {}\n{}\nRead the output file directly to view \
         the captured output.\nTo stop the now-background process: `task_stop({{ task_id: '{}' \
         }})`.",
        job.command,
        job.id,
        job.pid,
        job.output.display(),
        status_guidance(&job.status_file),
        job.id
    )
}

/// What `task_stop` answers a running job (`tools/task-stop.ts:98-117`),
/// its dialog dropped.
#[must_use]
pub fn stopping(job: &Job) -> String {
    format!(
        "Cancellation requested for background shell \"{}\"; captured output remains at {}.\n\
         Command: {}",
        job.id,
        job.output.display(),
        job.command
    )
}

/// What `task_stop` answers an id that names no job (`task-stop.ts:208`).
#[must_use]
pub fn not_found(id: &str) -> String {
    format!("Error: No background task found with ID \"{id}\".")
}

/// What `task_stop` answers a job no longer running (`task-stop.ts:224`).
#[must_use]
pub fn not_running(job: &Job) -> String {
    format!(
        "Error: Background shell \"{}\" is not running (status: {}).",
        job.id,
        job.status.word()
    )
}

/// The most of a job's output its notification carries, Qwen Code's
/// (`backgroundShellRegistry.ts:37`).
pub const TAIL_BYTES: usize = 8192;

/// The notification an ended job's model receives
/// (`backgroundShellRegistry.ts:497-570`): its id, status, command, working
/// directory, pid, exit status, its output's tail and the output file.
#[must_use]
pub fn notification(job: &Job, output: &[u8]) -> String {
    let said = match job.status {
        Status::Completed => "completed",
        Status::Failed => "failed",
        Status::Running | Status::Cancelled => "was cancelled",
    };
    let mut parts = vec![
        "<task-notification>".to_owned(),
        format!("<task-id>{}</task-id>", escaped(&job.id)),
        "<kind>shell</kind>".to_owned(),
        format!("<status>{}</status>", job.status.word()),
        format!(
            "<summary>Shell command \"{}\" {said}.</summary>",
            escaped(&job.command)
        ),
        format!("<command>{}</command>", escaped(&job.command)),
        format!("<cwd>{}</cwd>", escaped(&job.cwd)),
        format!("<pid>{}</pid>", job.pid),
    ];
    if let Some(code) = job.exit {
        parts.push(format!("<exit-code>{code}</exit-code>"));
    }
    if !output.is_empty() {
        let truncated = output.len() > TAIL_BYTES;
        let tail = String::from_utf8_lossy(&output[output.len().saturating_sub(TAIL_BYTES)..]);
        parts.push(format!(
            "<output-tail truncated=\"{truncated}\">{}</output-tail>",
            escaped(&tail)
        ));
    }
    parts.push(format!(
        "<output-file>{}</output-file>",
        escaped(&job.output.to_string_lossy())
    ));
    parts.push("</task-notification>".to_owned());
    parts.join("\n")
}

/// `text` safe inside the notification's elements.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Signal job `pid`'s whole process group: `TERM`, then -- Qwen Code's
/// grace, 200 ms later, on a thread of its own -- `KILL`.
pub fn stop_group(pid: u32) {
    let group = format!("-{pid}");
    let _ = std::process::Command::new("/bin/kill")
        .args(["-TERM", "--", &group])
        .output();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &group])
            .output();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_trailing_amp_comes_off_and_nothing_else_does() {
        assert_eq!(without_trailing_amp("npm run dev &"), "npm run dev");
        assert_eq!(without_trailing_amp("npm run dev &  "), "npm run dev");
        assert_eq!(without_trailing_amp("a && b"), "a && b");
        assert_eq!(without_trailing_amp("echo \\&"), "echo \\&");
        assert_eq!(without_trailing_amp("sleep 5"), "sleep 5");
    }

    #[test]
    fn a_job_id_is_bg_and_eight_hex_digits_and_never_repeats() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert!(a.starts_with("bg_") && a.len() == 11, "{a}");
        assert!(a[3..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_notification_carries_the_jobs_facts_and_escapes_them() {
        let job = Job {
            id: "bg_0123abcd".to_owned(),
            command: "echo <hi> & done".to_owned(),
            cwd: "~/git/x".to_owned(),
            pid: 42,
            output: PathBuf::from("/r/background/shell-bg_0123abcd.output"),
            status_file: PathBuf::from("/r/background/shell-bg_0123abcd.status"),
            status: Status::Completed,
            exit: Some(0),
            stopping: false,
        };
        let note = notification(&job, b"hi\n");
        assert!(note.starts_with("<task-notification>\n<task-id>bg_0123abcd</task-id>"));
        assert!(note.contains(
            "<summary>Shell command \"echo &lt;hi&gt; &amp; done\" completed.</summary>"
        ));
        assert!(note.contains("<exit-code>0</exit-code>"));
        assert!(note.contains("<output-tail truncated=\"false\">hi\n</output-tail>"));
        assert!(note.ends_with("</task-notification>"));
        assert_eq!(
            job.status_text(),
            "{\"id\":\"bg_0123abcd\",\"status\":\"completed\",\"pid\":42,\"exit_code\":0}\n"
        );
    }
}
