use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::config::Config;
use crate::ipc::protocol::{HookMessage, MessageKind};
use crate::kitty::{self, KittyClient};
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
            let icon = config.icon_for(state);
            let (active, inactive) = config.colors_for(state);
            kitty::apply(client.as_ref(), &msg.target, &icon, &active, &inactive);
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
                tracing::info!(%session_id, ?state, "session state updated");
                table.insert(
                    session_id,
                    Session {
                        state,
                        idle_timer,
                        resume_watch,
                    },
                );
            }
        }
        MessageKind::Cleanup => {
            kitty::clear(client.as_ref(), &msg.target);
            if let Some(session_id) = msg.session_id
                && let Some(old) = sessions.lock().await.remove(&session_id)
            {
                abort_timers(&old);
                tracing::info!(%session_id, "session cleaned up");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty::WindowTarget;
    use crate::kitty::fake::FakeKittyClient;

    fn harness() -> (Arc<Mutex<SessionTable>>, Arc<FakeKittyClient>, Arc<Config>) {
        (
            Arc::new(Mutex::new(SessionTable::new())),
            Arc::new(FakeKittyClient::new(vec![])),
            Arc::new(Config::default()),
        )
    }

    fn as_trait_object(fake: &Arc<FakeKittyClient>) -> Arc<dyn KittyClient + Send + Sync> {
        fake.clone()
    }

    fn set_state_msg(session_id: &str, state: State) -> HookMessage {
        HookMessage {
            session_id: Some(session_id.to_string()),
            target: WindowTarget::Id("1".to_string()),
            kind: MessageKind::SetState(state),
        }
    }

    #[tokio::test]
    async fn permission_arms_resume_watch_but_not_idle_timer() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        let table = sessions.lock().await;
        let session = table.get("s1").unwrap();
        assert_eq!(session.state, State::Permission);
        assert!(session.resume_watch.is_some());
        assert!(session.idle_timer.is_none());
    }

    #[tokio::test]
    async fn waiting_arms_both_timers() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Waiting),
            &sessions,
            &client,
            &config,
        )
        .await;

        let table = sessions.lock().await;
        let session = table.get("s1").unwrap();
        assert!(session.resume_watch.is_some());
        assert!(session.idle_timer.is_some());
    }

    #[tokio::test]
    async fn working_arms_neither_timer() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Working),
            &sessions,
            &client,
            &config,
        )
        .await;

        let table = sessions.lock().await;
        let session = table.get("s1").unwrap();
        assert!(session.resume_watch.is_none());
        assert!(session.idle_timer.is_none());
    }

    #[tokio::test]
    async fn fresh_message_aborts_previous_session_timers() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Waiting),
            &sessions,
            &client,
            &config,
        )
        .await;

        let (old_idle, old_resume) = {
            let table = sessions.lock().await;
            let session = table.get("s1").unwrap();
            (
                session.idle_timer.as_ref().unwrap().abort_handle(),
                session.resume_watch.as_ref().unwrap().abort_handle(),
            )
        };
        assert!(!old_idle.is_finished());
        assert!(!old_resume.is_finished());

        // A fresh message for the same session should retire both.
        apply(
            set_state_msg("s1", State::Done),
            &sessions,
            &client,
            &config,
        )
        .await;

        // Give the aborted tasks a moment to actually stop.
        tokio::task::yield_now().await;
        assert!(old_idle.is_finished());
        assert!(old_resume.is_finished());
    }

    #[tokio::test]
    async fn cleanup_removes_session_and_updates_tab() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Working),
            &sessions,
            &client,
            &config,
        )
        .await;

        let msg = HookMessage {
            session_id: Some("s1".to_string()),
            target: WindowTarget::Id("1".to_string()),
            kind: MessageKind::Cleanup,
        };
        apply(msg, &sessions, &client, &config).await;

        assert!(sessions.lock().await.get("s1").is_none());
        assert_eq!(fake.last_title(), Some(String::new()));
    }
}
