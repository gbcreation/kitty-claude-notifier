use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::ipc::protocol::HookMessage;
use crate::kitty::KittyClient;

use super::session::SessionTable;
use super::transitions;

/// Accepts connections forever, one lightweight task per hook invocation
/// (each connects, sends one line, and disconnects). Config is reloaded
/// fresh from disk for every message — editing config.toml takes effect
/// on the very next hook event, no daemon restart needed.
pub async fn run(
    listener: UnixListener,
    client: Arc<dyn KittyClient + Send + Sync>,
    config_path: PathBuf,
) -> Result<()> {
    let sessions: Arc<Mutex<SessionTable>> = Arc::new(Mutex::new(SessionTable::new()));
    loop {
        let (stream, _) = listener.accept().await?;
        let client = client.clone();
        let config_path = config_path.clone();
        let sessions = sessions.clone();
        tokio::spawn(async move {
            handle_connection(stream, client, config_path, sessions).await;
        });
    }
}

async fn handle_connection(
    stream: UnixStream,
    client: Arc<dyn KittyClient + Send + Sync>,
    config_path: PathBuf,
    sessions: Arc<Mutex<SessionTable>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Ok(msg) = serde_json::from_str::<HookMessage>(line.trim_end()) {
                    let config = Arc::new(load_config(&config_path));
                    transitions::apply(msg, &sessions, &client, &config).await;
                }
            }
        }
    }
}

fn load_config(path: &Path) -> Config {
    match Config::load(path) {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("failed to load config, using defaults for this message: {e}");
            Config::default()
        }
    }
}
