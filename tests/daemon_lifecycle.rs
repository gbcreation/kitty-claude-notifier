//! Integration test: spawns the real compiled daemon binary against an
//! isolated $HOME (a tempdir), sends it a real message over its Unix
//! socket, and confirms it processed it — without touching the developer's
//! actual ~/.config/kitty-claude-notifier or a real Kitty instance.
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Daemon {
    child: Child,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_daemon(home: &std::path::Path) -> Daemon {
    let child = Command::new(env!("CARGO_BIN_EXE_kitty-claude-notifier"))
        .arg("daemon")
        .env("HOME", home)
        .env_remove("KITTY_LISTEN_ON")
        .spawn()
        .expect("failed to spawn daemon binary");
    Daemon { child }
}

fn wait_for<F: Fn() -> bool>(deadline: Duration, check: F) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn daemon_binds_socket_and_processes_a_message() {
    let home = tempfile::tempdir().unwrap();
    let socket_path = home
        .path()
        .join(".config/kitty-claude-notifier/daemon.sock");
    let log_path = home.path().join(".config/kitty-claude-notifier/daemon.log");

    let _daemon = spawn_daemon(home.path());

    assert!(
        wait_for(Duration::from_secs(3), || socket_path.exists()),
        "daemon never bound its socket"
    );

    let mut stream = UnixStream::connect(&socket_path).expect("failed to connect to daemon");
    let msg = r#"{"session_id":"itest-1","target":{"Id":"999999"},"kind":{"SetState":"working"}}"#;
    stream.write_all(msg.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    drop(stream);

    assert!(
        wait_for(Duration::from_secs(2), || {
            std::fs::read_to_string(&log_path)
                .map(|log| log.contains("itest-1"))
                .unwrap_or(false)
        }),
        "daemon never logged processing the message"
    );

    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("daemon listening"));
    assert!(log.contains("itest-1"));
}
