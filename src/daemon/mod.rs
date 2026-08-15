mod idle_timer;
mod resume_watch;
mod server;
mod session;
mod transitions;

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UnixListener;

use crate::config::Config;
use crate::kitty::{KittyClient, ProcessKittyClient};
use crate::paths;

/// Entry point for the `daemon` subcommand — builds a small tokio runtime
/// just for this (other subcommands stay synchronous) and runs forever.
pub fn run(socket_path: &Path, config: Config) -> Result<()> {
    init_logging()?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async_run(socket_path, config))
}

fn init_logging() -> Result<()> {
    let log_path = paths::daemon_log_path();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_ansi(false)
        .with_target(false)
        .init();
    Ok(())
}

async fn async_run(socket_path: &Path, config: Config) -> Result<()> {
    // Held for the rest of this function's lifetime (the daemon's entire
    // run), and released automatically by the kernel if this process dies
    // for any reason — no stale-PID/lock-age guessing needed.
    let lock_path = paths::daemon_lock_path();
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;
    let mut lock = fd_lock::RwLock::new(lock_file);
    let _guard = match lock.try_write() {
        Ok(guard) => guard,
        Err(_) => {
            tracing::info!("another daemon instance already holds the lock; exiting");
            return Ok(());
        }
    };

    // Holding the lock proves no other daemon can be concurrently binding,
    // so any leftover socket file here is genuinely stale.
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    tracing::info!("daemon listening on {}", socket_path.display());

    let client: Arc<dyn KittyClient + Send + Sync> = Arc::new(ProcessKittyClient::new());
    server::run(listener, client, Arc::new(config)).await
}
