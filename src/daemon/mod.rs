mod idle_timer;
mod server;
mod session;
mod transitions;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UnixListener;

use crate::config::Config;
use crate::kitty::{KittyClient, ProcessKittyClient};

/// Entry point for the `daemon` subcommand — builds a small tokio runtime
/// just for this (other subcommands stay synchronous) and runs forever.
pub fn run(socket_path: &Path, config: Config) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async_run(socket_path, config))
}

async fn async_run(socket_path: &Path, config: Config) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Minimal stale-socket handling: if nothing answers, the file is from a
    // dead daemon and safe to remove before binding. Not race-free against a
    // second daemon starting concurrently — hardened with a real lock later.
    if socket_path.exists() && std::os::unix::net::UnixStream::connect(socket_path).is_err() {
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    let client: Arc<dyn KittyClient + Send + Sync> = Arc::new(ProcessKittyClient::new());
    server::run(listener, client, Arc::new(config)).await
}
