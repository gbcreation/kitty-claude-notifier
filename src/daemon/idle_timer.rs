use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::kitty::{self, KittyClient, WindowTarget};
use crate::state::State;

use super::session::SessionTable;

/// Spawns a cancellable timer that fires `Done`/`Waiting`/`Compacting` ->
/// `Idle` at the exact configured duration, no polling, unlike the bash
/// daemon's fixed 10s tick. Caller is responsible for aborting the
/// previous timer (if any) before calling this again for the same
/// session.
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
            // No state currently arms both idle_timer and resume_watch at
            // once (only Permission arms resume_watch, and it never arms
            // idle_timer), but clear it defensively in case that ever
            // changes; a resume_watch task left dangling here would keep
            // polling for a session idle_timer just retired.
            if let Some(handle) = session.resume_watch.take() {
                handle.abort();
            }
            session.state = State::Idle;
            session.idle_timer = None;
            let visual = State::Idle.visual(!session.active_agents.is_empty());
            tracing::info!(%session_id, "idle timeout reached");
            let icon = config.icon_for(visual);
            let (active, inactive) = config.colors_for(visual);
            kitty::apply(client.as_ref(), &target, &icon, &active, &inactive);
        }
    })
}
