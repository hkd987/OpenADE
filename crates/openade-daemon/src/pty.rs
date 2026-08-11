//! PTY host (PRD R1).
//!
//! Harness CLIs run inside real PTYs owned by the daemon, tmux-style: the
//! desktop app is just a viewer that attaches over the local API, so killing
//! and reopening the window reattaches every session with full scrollback.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use uuid::Uuid;

/// Maximum bytes of scrollback retained per session (oldest dropped first).
pub const SCROLLBACK_LIMIT: usize = 2 * 1024 * 1024;

/// A command to run inside a PTY.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables (inherits the daemon's env otherwise).
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>) -> Self {
        CommandSpec {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Errors from PTY operations.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("failed to open pty: {0}")]
    Open(String),
    #[error("failed to spawn {program:?}: {message}")]
    Spawn { program: String, message: String },
    #[error("no such pty session: {0}")]
    NotFound(Uuid),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

struct Scrollback {
    buf: Vec<u8>,
}

impl Scrollback {
    fn append(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        if self.buf.len() > SCROLLBACK_LIMIT {
            let excess = self.buf.len() - SCROLLBACK_LIMIT;
            self.buf.drain(..excess);
        }
    }
}

/// One live (or exited) PTY session.
pub struct PtySession {
    id: Uuid,
    scrollback: Arc<Mutex<Scrollback>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    exited: Arc<AtomicBool>,
    exit_code: Arc<Mutex<Option<u32>>>,
    last_output: Arc<Mutex<std::time::Instant>>,
}

impl PtySession {
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Snapshot of the retained output (lossy UTF-8).
    pub fn scrollback(&self) -> String {
        let sb = self.scrollback.lock().expect("scrollback lock");
        String::from_utf8_lossy(&sb.buf).into_owned()
    }

    /// Bytes of retained output.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.lock().expect("scrollback lock").buf.len()
    }

    /// The last `max_bytes` of output (lossy UTF-8) — used for prompt
    /// detection.
    pub fn tail(&self, max_bytes: usize) -> String {
        let sb = self.scrollback.lock().expect("scrollback lock");
        let start = sb.buf.len().saturating_sub(max_bytes);
        String::from_utf8_lossy(&sb.buf[start..]).into_owned()
    }

    /// How long the PTY has been silent (no output).
    pub fn idle_for(&self) -> std::time::Duration {
        self.last_output.lock().expect("last_output lock").elapsed()
    }

    /// Whether the child process has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// The child's exit code, once it has been reaped.
    pub fn exit_code(&self) -> Option<u32> {
        *self.exit_code.lock().expect("exit_code lock")
    }

    /// Send input to the child as if typed.
    pub fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut w = self.writer.lock().expect("writer lock");
        w.write_all(data)?;
        w.flush()?;
        Ok(())
    }

    /// Resize the terminal.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.master
            .lock()
            .expect("master lock")
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))
    }

    /// Kill the child process.
    pub fn kill(&self) -> Result<(), PtyError> {
        self.killer
            .lock()
            .expect("killer lock")
            .kill()
            .map_err(PtyError::Io)
    }
}

/// Strip ANSI escape sequences (CSI and OSC) so prompt detection sees what
/// the user sees.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ params... final byte in @..=~
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... terminated by BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{07}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-char escapes (ESC c, ESC =, ...)
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Heuristic: does the terminal tail look like the process is waiting for
/// the user (R1 `needs-input` state)? Checked only after output quiescence.
pub fn looks_like_awaiting_input(tail: &str) -> bool {
    let clean = strip_ansi(tail);
    let Some(last_line) = clean.lines().rev().find(|l| !l.trim().is_empty()) else {
        return false;
    };
    let line = last_line.trim_end();
    let lower = line.to_lowercase();

    const PHRASES: [&str; 7] = [
        "(y/n)",
        "[y/n]",
        "[y/n/a]",
        "press enter",
        "password",
        "continue?",
        "proceed?",
    ];
    if PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // Interactive prompts overwhelmingly end in one of these.
    const PROMPT_ENDINGS: [char; 7] = ['?', ':', '>', '$', '#', '❯', '›'];
    line.chars()
        .last()
        .is_some_and(|c| PROMPT_ENDINGS.contains(&c))
}

/// Owns all PTY sessions in the daemon.
#[derive(Default)]
pub struct PtyHost {
    sessions: Mutex<HashMap<Uuid, Arc<PtySession>>>,
}

impl PtyHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn `spec` in a fresh PTY with `cwd` as working directory, tracked
    /// under `session_id`.
    pub fn spawn(
        &self,
        session_id: Uuid,
        spec: &CommandSpec,
        cwd: Option<PathBuf>,
    ) -> Result<Arc<PtySession>, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 32,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| PtyError::Spawn {
            program: spec.program.clone(),
            message: e.to_string(),
        })?;
        // Close our copy of the slave end so the reader sees EOF on exit.
        drop(pair.slave);
        let killer = child.clone_killer();

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Open(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let scrollback = Arc::new(Mutex::new(Scrollback { buf: Vec::new() }));
        let exited = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(Mutex::new(None));

        // Waiter thread: reaps the child (no zombies) and records its exit.
        {
            let exited = Arc::clone(&exited);
            let exit_code = Arc::clone(&exit_code);
            std::thread::Builder::new()
                .name(format!("pty-waiter-{session_id}"))
                .spawn(move || {
                    if let Ok(status) = child.wait() {
                        *exit_code.lock().expect("exit_code lock") = Some(status.exit_code());
                    }
                    exited.store(true, Ordering::SeqCst);
                })?;
        }

        // Reader thread: drains the PTY into the scrollback buffer.
        let last_output = Arc::new(Mutex::new(std::time::Instant::now()));
        {
            let scrollback = Arc::clone(&scrollback);
            let last_output = Arc::clone(&last_output);
            std::thread::Builder::new()
                .name(format!("pty-reader-{session_id}"))
                .spawn(move || {
                    let mut buf = [0u8; 8192];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                scrollback
                                    .lock()
                                    .expect("scrollback lock")
                                    .append(&buf[..n]);
                                *last_output.lock().expect("last_output lock") =
                                    std::time::Instant::now();
                            }
                        }
                    }
                })?;
        }

        let session = Arc::new(PtySession {
            id: session_id,
            scrollback,
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
            exited,
            exit_code,
            last_output,
        });

        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(session_id, Arc::clone(&session));
        Ok(session)
    }

    pub fn get(&self, id: Uuid) -> Result<Arc<PtySession>, PtyError> {
        self.sessions
            .lock()
            .expect("sessions lock")
            .get(&id)
            .cloned()
            .ok_or(PtyError::NotFound(id))
    }

    pub fn ids(&self) -> Vec<Uuid> {
        self.sessions
            .lock()
            .expect("sessions lock")
            .keys()
            .copied()
            .collect()
    }

    /// Kill (best-effort) and forget a session.
    pub fn remove(&self, id: Uuid) -> Result<(), PtyError> {
        let session = self
            .sessions
            .lock()
            .expect("sessions lock")
            .remove(&id)
            .ok_or(PtyError::NotFound(id))?;
        let _ = session.kill();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_until(deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    #[test]
    fn spawn_captures_output_and_exit() {
        let host = PtyHost::new();
        let id = Uuid::new_v4();
        let spec = CommandSpec::new("sh")
            .arg("-c")
            .arg("printf 'hello-from-pty'; exit 0");
        let session = host.spawn(id, &spec, None).unwrap();

        assert!(wait_until(Duration::from_secs(10), || session.has_exited()));
        assert!(
            session.scrollback().contains("hello-from-pty"),
            "scrollback was: {:?}",
            session.scrollback()
        );
    }

    #[test]
    fn input_reaches_the_child() {
        let host = PtyHost::new();
        let id = Uuid::new_v4();
        // `head -n1` echoes (PTY echo) and exits after one line.
        let spec = CommandSpec::new("sh")
            .arg("-c")
            .arg("read line; echo \"got:$line\"");
        let session = host.spawn(id, &spec, None).unwrap();

        session.write_input(b"ping\n").unwrap();
        assert!(wait_until(Duration::from_secs(10), || session.has_exited()));
        assert!(
            session.scrollback().contains("got:ping"),
            "{:?}",
            session.scrollback()
        );
    }

    #[test]
    fn sessions_survive_in_host_and_can_be_killed() {
        let host = PtyHost::new();
        let id = Uuid::new_v4();
        let spec = CommandSpec::new("sh").arg("-c").arg("sleep 30");
        let session = host.spawn(id, &spec, None).unwrap();
        assert!(!session.has_exited());
        assert_eq!(host.ids(), vec![id]);

        host.remove(id).unwrap();
        assert!(host.get(id).is_err());
        assert!(wait_until(Duration::from_secs(10), || session.has_exited()));
    }

    #[test]
    fn cwd_and_env_are_applied() {
        let host = PtyHost::new();
        let tmp = tempfile::tempdir().unwrap();
        let spec = CommandSpec::new("sh")
            .arg("-c")
            .arg("pwd; printf \"%s\" \"$OPENADE_TEST\"")
            .env("OPENADE_TEST", "env-ok");
        let session = host
            .spawn(Uuid::new_v4(), &spec, Some(tmp.path().to_path_buf()))
            .unwrap();
        assert!(wait_until(Duration::from_secs(10), || session.has_exited()));
        let out = session.scrollback();
        let canon = tmp.path().canonicalize().unwrap();
        assert!(
            out.contains(tmp.path().to_str().unwrap()) || out.contains(canon.to_str().unwrap()),
            "{out:?}"
        );
        assert!(out.contains("env-ok"), "{out:?}");
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        assert_eq!(strip_ansi("\u{1b}[1;32mhello\u{1b}[0m"), "hello");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{07}prompt> "), "prompt> ");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[2J\u{1b}[Hcleared"), "cleared");
    }

    #[test]
    fn prompt_detection_heuristics() {
        for tail in [
            "Do you want to continue? (y/n) ",
            "Overwrite CLAUDE.md? [Y/n]",
            "Enter password: ",
            "some output\n$ ",
            "❯ ",
            "\u{1b}[32m?\u{1b}[0m Pick a model >",
            "Press Enter to continue",
        ] {
            assert!(looks_like_awaiting_input(tail), "should detect: {tail:?}");
        }
        for tail in [
            "",
            "Compiling openade-core v0.1.0",
            "running 5 tests\ntest a ... ok",
            "downloaded 3 crates in 1.2s.",
        ] {
            assert!(!looks_like_awaiting_input(tail), "false positive: {tail:?}");
        }
    }

    #[test]
    fn idle_and_tail_reflect_output_activity() {
        let host = PtyHost::new();
        let spec = CommandSpec::new("sh")
            .arg("-c")
            .arg("printf 'Continue? (y/n) '; read x; echo done");
        let session = host.spawn(Uuid::new_v4(), &spec, None).unwrap();

        assert!(wait_until(Duration::from_secs(10), || {
            session.tail(64).contains("(y/n)")
        }));
        // Quiesces once the prompt is printed.
        assert!(wait_until(Duration::from_secs(10), || {
            session.idle_for() >= Duration::from_millis(300)
        }));
        assert!(looks_like_awaiting_input(&session.tail(64)));

        session.write_input(b"y\n").unwrap();
        assert!(wait_until(Duration::from_secs(10), || session.has_exited()));
    }

    #[test]
    fn scrollback_is_capped() {
        let mut sb = Scrollback { buf: Vec::new() };
        sb.append(&vec![b'a'; SCROLLBACK_LIMIT]);
        sb.append(b"tail");
        assert_eq!(sb.buf.len(), SCROLLBACK_LIMIT);
        assert!(sb.buf.ends_with(b"tail"));
    }
}
