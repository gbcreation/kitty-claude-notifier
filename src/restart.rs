use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

use crate::ipc::connect::connect_or_spawn_daemon;

/// How long to wait for a signaled daemon to actually exit before giving
/// up. Generous: the daemon's own shutdown is instant (no graceful
/// drain), so this only needs to cover process-scheduling delay.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Stops whatever daemon is currently running (if any) and starts a
/// fresh one, so a rebuilt/reinstalled binary or a changed environment
/// (e.g. a new `KITTY_LISTEN_ON` after Kitty itself restarted) takes
/// effect immediately, without the user having to find and kill the old
/// process by hand first.
pub fn run(socket_path: &Path, pid_path: &Path) -> Result<()> {
    match read_running_pid(pid_path) {
        Some(pid) => {
            println!("kitty-claude-notifier: stopping daemon (pid {pid})...");
            stop(pid, pid_path)?;
        }
        None if UnixStream::connect(socket_path).is_ok() => {
            // A daemon is actually listening, but we have no way to
            // signal it (no daemon.pid, e.g. a still-running instance
            // spawned by a build that predates this file). Without this
            // check, connect_or_spawn_daemon below would just silently
            // reconnect to *that* old daemon and report success without
            // having restarted anything at all.
            anyhow::bail!(
                "a daemon is running at {} but its pid is unknown (daemon.pid \
                 missing or stale, likely an older binary); stop it manually \
                 (e.g. `pkill -f 'kitty-claude-notifier daemon'`) and run \
                 restart again",
                socket_path.display()
            );
        }
        None => println!("kitty-claude-notifier: no running daemon found"),
    }

    println!("kitty-claude-notifier: starting daemon...");
    connect_or_spawn_daemon(socket_path).context("failed to start a fresh daemon")?;
    println!("kitty-claude-notifier: daemon restarted");
    Ok(())
}

/// The pid recorded in `pid_path`, but only if a process with that pid is
/// actually still alive: this tool doesn't reliably remove the file on
/// every daemon exit path (e.g. a crash, or being killed directly), so a
/// stale entry must never be mistaken for a live daemon.
fn read_running_pid(pid_path: &Path) -> Option<i32> {
    let raw = std::fs::read_to_string(pid_path).ok()?;
    let pid: i32 = raw.trim().parse().ok()?;
    is_alive(pid).then_some(pid)
}

fn is_alive(pid: i32) -> bool {
    signal::kill(Pid::from_raw(pid), None).is_ok()
}

fn stop(pid: i32, pid_path: &Path) -> Result<()> {
    signal::kill(Pid::from_raw(pid), Signal::SIGTERM)
        .context("failed to signal the running daemon")?;

    let deadline = std::time::Instant::now() + STOP_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if !is_alive(pid) {
            // Best-effort: the next daemon to win the lock overwrites
            // this file regardless, so a failed removal here is harmless.
            let _ = std::fs::remove_file(pid_path);
            return Ok(());
        }
        sleep(POLL_INTERVAL);
    }
    anyhow::bail!(
        "daemon (pid {pid}) did not exit within {}s of SIGTERM",
        STOP_TIMEOUT.as_secs()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn spawn_sleep() -> std::process::Child {
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn `sleep` for the test")
    }

    /// A signaled process lingers as a zombie (still visible to
    /// `kill(pid, 0)`) until its parent reaps it. The real daemon is
    /// always reparented to init/systemd by the time `restart` signals
    /// it, which reaps it promptly; here the test process is the direct
    /// parent, so it must reap concurrently itself for `is_alive` to
    /// ever observe the process as gone.
    fn spawn_sleep_with_background_reaper() -> i32 {
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        pid
    }

    #[test]
    fn read_running_pid_is_none_when_the_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_running_pid(&dir.path().join("daemon.pid")), None);
    }

    #[test]
    fn read_running_pid_is_none_for_a_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        let mut child = spawn_sleep();
        let pid = child.id();
        child.kill().unwrap();
        child.wait().unwrap();

        std::fs::write(&pid_path, pid.to_string()).unwrap();
        assert_eq!(read_running_pid(&pid_path), None);
    }

    #[test]
    fn read_running_pid_is_some_for_a_live_pid() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        let mut child = spawn_sleep();
        std::fs::write(&pid_path, child.id().to_string()).unwrap();

        assert_eq!(read_running_pid(&pid_path), Some(child.id() as i32));

        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn stop_terminates_a_running_process_and_removes_the_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        let pid = spawn_sleep_with_background_reaper();
        std::fs::write(&pid_path, pid.to_string()).unwrap();

        stop(pid, &pid_path).unwrap();

        assert!(!is_alive(pid));
        assert!(!pid_path.exists());
    }

    #[test]
    fn stop_on_an_already_dead_pid_errors_rather_than_hanging() {
        // Guards against silently declaring victory over a pid that was
        // never signaled successfully in the first place (e.g. reused by
        // an unrelated process by the time `stop` runs).
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        child.kill().unwrap();
        child.wait().unwrap();

        // A freshly-reaped pid is very unlikely to have been reused yet,
        // so signaling it is expected to fail with ESRCH.
        assert!(stop(pid, &pid_path).is_err());
    }

    #[test]
    fn run_refuses_to_claim_success_when_a_daemon_is_listening_with_no_known_pid() {
        // Regression: a daemon.pid missing or stale (e.g. a still-running
        // instance spawned by a build that predates this file) must not
        // let `run` silently reconnect to that old daemon via
        // connect_or_spawn_daemon's fast path and report a bogus success.
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("daemon.sock");
        let pid_path = dir.path().join("daemon.pid");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

        let err = run(&socket_path, &pid_path).unwrap_err();
        assert!(err.to_string().contains("pid is unknown"));
    }
}
