use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::kitty::{KittyClient, WindowTarget};
use crate::state::State;

use super::session::SessionTable;

/// Spawns a cancellable timer that fires `Done`/`Waiting` -> `Idle` at the
/// exact configured duration — no polling, unlike the bash daemon's fixed
/// 10s tick. Caller is responsible for aborting the previous timer (if any)
/// before calling this again for the same session.
pub fn spawn(
    session_id: String,
    target: WindowTarget,
    duration: Duration,
    sessions: Arc<Mutex<SessionTable>>,
    client: Arc<dyn KittyClient + Send + Sync>,
    config: Arc<Config>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(duration).await;
        let mut table = sessions.lock().await;
        if let Some(session) = table.get_mut(&session_id) {
            session.state = State::Idle;
            session.idle_timer = None;
            let _ = client.set_tab_title(&target, &config.title_for(State::Idle));
            let _ = client.set_tab_color(&target, &config.color_for(State::Idle));
        }
    })
}
