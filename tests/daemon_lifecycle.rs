//! Integration test: spawns the real compiled daemon binary against an
//! isolated $HOME (a tempdir), sends it a real message over its Unix
//! socket, and confirms it processed it, without touching the developer's
//! actual ~/.config/kitty-claude-notifier or a real Kitty instance.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
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

/// Spawns the daemon with a mock `kitten` on PATH ahead of the real one, so
/// its actual set-tab-title/set-tab-color invocations can be inspected.
fn spawn_daemon_with_mock_kitten(
    home: &std::path::Path,
    mock_dir: &std::path::Path,
    mock_log: &std::path::Path,
) -> Daemon {
    let script_path = mock_dir.join("kitten");
    fs::write(&script_path, include_str!("support/mock_kitten.sh")).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let original_path = std::env::var("PATH").unwrap_or_default();
    let child = Command::new(env!("CARGO_BIN_EXE_kitty-claude-notifier"))
        .arg("daemon")
        .env("HOME", home)
        .env("PATH", format!("{}:{original_path}", mock_dir.display()))
        .env("MOCK_KITTEN_LOG", mock_log)
        .env("MOCK_KITTEN_LS_TITLE", "my project")
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

    let msg = r#"{"session_id":"itest-1","target":{"Id":"999999"},"kind":{"SetState":"working"}}"#;
    send_message(&socket_path, msg);

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

fn send_message(socket_path: &std::path::Path, msg: &str) {
    let mut stream = UnixStream::connect(socket_path).expect("failed to connect to daemon");
    stream.write_all(msg.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
}

/// Regression test: config.toml used to be loaded exactly once at daemon
/// startup, so an edit made after the daemon was already running had no
/// effect until it was killed and respawned. server::run now reloads it
/// fresh per message, proving an edit takes effect on the very next
/// hook event, no restart required.
#[test]
fn daemon_reloads_config_without_restarting() {
    let home = tempfile::tempdir().unwrap();
    let mock_dir = tempfile::tempdir().unwrap();
    let mock_log = home.path().join("kitten_calls.log");
    let socket_path = home
        .path()
        .join(".config/kitty-claude-notifier/daemon.sock");
    let config_path = home
        .path()
        .join(".config/kitty-claude-notifier/config.toml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();

    fs::write(
        &config_path,
        r##"
        [icons.working]
        glyph = "X"
        color = "#111111"
        "##,
    )
    .unwrap();

    let _daemon = spawn_daemon_with_mock_kitten(home.path(), mock_dir.path(), &mock_log);
    assert!(
        wait_for(Duration::from_secs(3), || socket_path.exists()),
        "daemon never bound its socket"
    );

    let msg = r#"{"session_id":"reload-1","target":{"Id":"1"},"kind":{"SetState":"working"}}"#;
    send_message(&socket_path, msg);
    assert!(
        wait_for(Duration::from_secs(2), || {
            fs::read_to_string(&mock_log)
                .map(|l| l.contains('X'))
                .unwrap_or(false)
        }),
        "first message never used the initial config's icon"
    );
    // "#111111" as decimal RGB: the ANSI escape encodes color as
    // decimal, not the literal hex string.
    let first_log = fs::read_to_string(&mock_log).unwrap();
    assert!(first_log.contains("38;2;17;17;17"));

    // Edit config.toml while the daemon is still running, no restart.
    fs::write(
        &config_path,
        r##"
        [icons.working]
        glyph = "Y"
        color = "#222222"
        "##,
    )
    .unwrap();

    send_message(&socket_path, msg);
    assert!(
        wait_for(Duration::from_secs(2), || {
            fs::read_to_string(&mock_log)
                .map(|l| l.contains('Y'))
                .unwrap_or(false)
        }),
        "second message never picked up the edited config's icon"
    );
    // "#222222" as decimal RGB, same reasoning as above.
    let second_log = fs::read_to_string(&mock_log).unwrap();
    assert!(second_log.contains("38;2;34;34;34"));
}
