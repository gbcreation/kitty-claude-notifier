use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::Config;
use crate::ipc::protocol::{HookMessage, MessageKind};
use crate::kitty::KittyClient;

use super::session::{Session, SessionTable};

/// Applies one incoming message: updates the tab via `client`, and reflects
/// the change in the in-memory session table (skipped if the message has no
/// session_id — the tab update still happens, matching the bash tool's
/// behavior of updating the tab unconditionally even without one).
pub async fn apply(
    msg: HookMessage,
    sessions: &Arc<Mutex<SessionTable>>,
    client: &(dyn KittyClient + Send + Sync),
    config: &Config,
) {
    match msg.kind {
        MessageKind::SetState(state) => {
            let _ = client.set_tab_title(&msg.target, &config.title_for(state));
            let _ = client.set_tab_color(&msg.target, &config.color_for(state));
            if let Some(session_id) = msg.session_id {
                sessions.lock().await.insert(
                    session_id,
                    Session {
                        state,
                        target: msg.target,
                        transcript_path: msg.transcript_path,
                    },
                );
            }
        }
        MessageKind::Cleanup => {
            let _ = client.set_tab_title(&msg.target, "");
            let _ = client.set_tab_color(&msg.target, "NONE");
            if let Some(session_id) = msg.session_id {
                sessions.lock().await.remove(&session_id);
            }
        }
    }
}
