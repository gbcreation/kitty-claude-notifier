use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::config::Config;
use crate::ipc::protocol::{HookMessage, MessageKind};
use crate::kitty::KittyClient;
use crate::state::State;

use super::idle_timer;
use super::resume_watch;
use super::session::{Session, SessionTable};

/// Applies one incoming message: updates the tab via `client`, and reflects
/// the change in the in-memory session table (skipped if the message has no
/// session_id — the tab update still happens, matching the bash tool's
/// behavior of updating the tab unconditionally even without one).
pub async fn apply(
    msg: HookMessage,
    sessions: &Arc<Mutex<SessionTable>>,
    client: &Arc<dyn KittyClient + Send + Sync>,
    config: &Arc<Config>,
) {
    match msg.kind {
        MessageKind::SetState(state) => {
            let _ = client.set_tab_title(&msg.target, &config.title_for(state));
            let _ = client.set_tab_color(&msg.target, &config.color_for(state));
            if let Some(session_id) = msg.session_id {
                let mut table = sessions.lock().await;
                // Any prior timers belong to a state this message
                // supersedes — abort them so neither can fire late.
                if let Some(old) = table.remove(&session_id) {
                    abort_timers(&old);
                }
                let idle_timer = matches!(state, State::Done | State::Waiting).then(|| {
                    idle_timer::spawn(
                        session_id.clone(),
                        msg.target.clone(),
                        Duration::from_secs(config.idle_timeout_secs),
                        sessions.clone(),
                        client.clone(),
                        config.clone(),
                    )
                });
                let resume_watch = matches!(state, State::Permission | State::Waiting).then(|| {
                    resume_watch::spawn(
                        session_id.clone(),
                        msg.target.clone(),
                        sessions.clone(),
                        client.clone(),
                        config.clone(),
                    )
                });
                table.insert(
                    session_id,
                    Session {
                        state,
                        target: msg.target,
                        transcript_path: msg.transcript_path,
                        idle_timer,
                        resume_watch,
                    },
                );
            }
        }
        MessageKind::Cleanup => {
            let _ = client.set_tab_title(&msg.target, "");
            let _ = client.set_tab_color(&msg.target, "NONE");
            if let Some(session_id) = msg.session_id {
                if let Some(old) = sessions.lock().await.remove(&session_id) {
                    abort_timers(&old);
                }
            }
        }
    }
}

fn abort_timers(session: &Session) {
    if let Some(handle) = &session.idle_timer {
        handle.abort();
    }
    if let Some(handle) = &session.resume_watch {
        handle.abort();
    }
}
