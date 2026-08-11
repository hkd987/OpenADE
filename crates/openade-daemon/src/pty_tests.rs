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
fn spawn_of_a_missing_program_is_an_error() {
    let host = PtyHost::new();
    let spec = CommandSpec::new("definitely-not-a-real-binary-xyz");
    assert!(host.spawn(Uuid::new_v4(), &spec, None).is_err());
    // Host-level errors for unknown ids.
    assert!(host.get(Uuid::new_v4()).is_err());
    assert!(host.remove(Uuid::new_v4()).is_err());
}

#[test]
fn resize_and_ids_work_on_a_live_session() {
    let host = PtyHost::new();
    let id = Uuid::new_v4();
    let spec = CommandSpec::new("sh").arg("-c").arg("sleep 30");
    let session = host.spawn(id, &spec, None).unwrap();
    assert_eq!(session.id(), id);
    session.resize(40, 100).unwrap();
    assert_eq!(host.ids(), vec![id]);
    assert_eq!(session.scrollback_len(), session.scrollback().len());
    host.remove(id).unwrap();
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

#[test]
fn strip_ansi_handles_esc_terminators_and_stray_escapes() {
    // OSC terminated by ESC-backslash (ST) instead of BEL.
    assert_eq!(strip_ansi("\u{1b}]0;title\u{1b}\\after"), "after");
    // Two-character escape sequences (ESC =, ESC c, ...).
    assert_eq!(strip_ansi("\u{1b}=keypad"), "keypad");
    // A trailing bare ESC at end of input.
    assert_eq!(strip_ansi("tail\u{1b}"), "tail");
    // OSC that runs to end of input without a terminator.
    assert_eq!(strip_ansi("\u{1b}]0;unterminated"), "");
}
