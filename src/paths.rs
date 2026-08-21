use std::path::PathBuf;

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home)
        .join(".config")
        .join("kitty-claude-notifier")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Where `install` copies the binary to, so registered hook commands keep
/// working even if the build directory (e.g. `target/debug/`) is cleaned.
pub fn installed_binary_path() -> PathBuf {
    config_dir().join("bin").join("kitty-claude-notifier")
}

pub fn daemon_socket_path() -> PathBuf {
    config_dir().join("daemon.sock")
}

/// A real OS advisory lock (held for the daemon's whole lifetime, released
/// automatically by the kernel on crash) gates daemon startup so two
/// concurrently-spawned daemons can't race to bind the socket.
pub fn daemon_lock_path() -> PathBuf {
    config_dir().join("daemon.lock")
}

pub fn daemon_log_path() -> PathBuf {
    config_dir().join("daemon.log")
}

/// Records the running daemon's own pid, written fresh every time it wins
/// `daemon_lock_path`'s lock. Lets `restart` find and signal it without
/// having to `pgrep` the process table; a stale entry from a crashed
/// daemon is harmless, since anything reading it must first confirm the
/// pid is still alive before trusting it.
pub fn daemon_pid_path() -> PathBuf {
    config_dir().join("daemon.pid")
}
