use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};

use super::protocol::HookMessage;

/// Sends one message to the daemon, spawning it first if nothing is
/// listening yet. This is the "fast path": no locking against concurrent
/// spawns yet (a known simplification; hardened in a later milestone).
pub fn send(socket_path: &Path, msg: &HookMessage) -> Result<()> {
    let mut stream = connect_or_spawn_daemon(socket_path)?;
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    Ok(())
}

fn connect_or_spawn_daemon(socket_path: &Path) -> Result<UnixStream> {
    if let Ok(stream) = UnixStream::connect(socket_path) {
        return Ok(stream);
    }
    spawn_daemon_detached()?;
    for _ in 0..20 {
        sleep(Duration::from_millis(50));
        if let Ok(stream) = UnixStream::connect(socket_path) {
            return Ok(stream);
        }
    }
    anyhow::bail!(
        "daemon did not start listening at {}",
        socket_path.display()
    )
}

fn spawn_daemon_detached() -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    Command::new(exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn daemon")?;
    Ok(())
}
