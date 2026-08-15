use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::Config;
use crate::kitty::{self, KittyClient, WindowTarget};
use crate::markers;
use crate::state::State;

use super::session::SessionTable;

/// Safety cap: if a permission/waiting prompt's markers never disappear
/// (e.g. Claude Code's wording drifted and no longer matches anything in
/// the config), stop polling rather than spin forever.
const MAX_WAIT: Duration = Duration::from_secs(600);

/// Consecutive clear polls required before committing the transition —
/// guards against a burst of permission prompts (approve one, immediately
/// hit another) being misread as full resolution from a single clear tick.
const CONFIRMATIONS_REQUIRED: u32 = 2;

/// Polls `kitten @ get-text` for `target` while a session sits in
/// permission/waiting, watching for the configured markers to disappear —
/// the reactive replacement for the bash tool's transcript-mtime heuristic.
pub fn spawn(
    session_id: String,
    target: WindowTarget,
    sessions: Arc<Mutex<SessionTable>>,
    client: Arc<dyn KittyClient + Send + Sync>,
    config: Arc<Config>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let poll_interval = Duration::from_millis(config.resume_poll_interval_ms);
        let deadline = Instant::now() + MAX_WAIT;
        let mut consecutive_clear = 0u32;
        let mut last_text = String::new();

        while Instant::now() < deadline {
            tokio::time::sleep(poll_interval).await;

            let text = match client.get_text(&target) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!(%session_id, "get_text failed, will retry: {e}");
                    continue;
                }
            };
            last_text = text.clone();

            if markers::any_present(&text, &config.permission_markers) {
                consecutive_clear = 0;
                continue;
            }

            consecutive_clear += 1;
            if consecutive_clear < CONFIRMATIONS_REQUIRED {
                continue;
            }

            let mut table = sessions.lock().await;
            if let Some(session) = table.get_mut(&session_id) {
                // A session in Waiting has both timers armed; resume_watch
                // winning the race means idle_timer's countdown no longer
                // applies.
                if let Some(handle) = session.idle_timer.take() {
                    handle.abort();
                }
                session.state = State::Working;
                session.resume_watch = None;
            }
            drop(table);
            tracing::info!(%session_id, "resume detected — permission/waiting cleared");
            let icon = config.icon_for(State::Working);
            let (active, inactive) = config.colors_for(State::Working);
            kitty::apply(client.as_ref(), &target, &icon, &active, &inactive);
            return;
        }

        tracing::warn!(
            %session_id,
            timeout = ?MAX_WAIT,
            "resume-watch timed out without detecting resolution; last screen text:\n{last_text}"
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty::fake::FakeKittyClient;

    use super::super::session::Session;

    fn config_with_interval(ms: u64) -> Arc<Config> {
        Arc::new(Config {
            resume_poll_interval_ms: ms,
            ..Config::default()
        })
    }

    async fn seed_session(sessions: &Arc<Mutex<SessionTable>>, id: &str, state: State) {
        sessions.lock().await.insert(
            id.to_string(),
            Session {
                state,
                idle_timer: None,
                resume_watch: None,
            },
        );
    }

    #[tokio::test(start_paused = true)]
    async fn resolves_after_two_consecutive_clear_polls() {
        let config = config_with_interval(100);
        let target = WindowTarget::Id("1".to_string());
        let fake = Arc::new(FakeKittyClient::new(vec![
            "do you want to proceed?",
            "do you want to proceed?",
            "cleared",
            "cleared",
        ]));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let sessions: Arc<Mutex<SessionTable>> = Arc::new(Mutex::new(SessionTable::new()));
        seed_session(&sessions, "s1", State::Permission).await;

        let handle = spawn(
            "s1".to_string(),
            target.clone(),
            sessions.clone(),
            client,
            config.clone(),
        );

        for _ in 0..4 {
            tokio::time::advance(Duration::from_millis(config.resume_poll_interval_ms)).await;
        }
        handle.await.unwrap();

        let table = sessions.lock().await;
        assert_eq!(table.get("s1").unwrap().state, State::Working);
        let icon = config.icon_for(State::Working);
        assert_eq!(
            fake.last_title(),
            Some(crate::icon::build_title(&icon.glyph, &icon.color, ""))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn marker_reappearing_resets_the_confirmation_count() {
        let config = config_with_interval(100);
        let target = WindowTarget::Id("1".to_string());
        // clear, present, clear, clear: must NOT resolve after the first
        // clear+present pair — only after two FRESH consecutive clears.
        let fake = Arc::new(FakeKittyClient::new(vec![
            "cleared",
            "do you want to proceed?",
            "cleared",
            "cleared",
        ]));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let sessions: Arc<Mutex<SessionTable>> = Arc::new(Mutex::new(SessionTable::new()));
        seed_session(&sessions, "s1", State::Permission).await;

        let handle = spawn(
            "s1".to_string(),
            target.clone(),
            sessions.clone(),
            client,
            config.clone(),
        );

        for _ in 0..2 {
            tokio::time::advance(Duration::from_millis(config.resume_poll_interval_ms)).await;
        }
        tokio::task::yield_now().await;
        assert!(
            !handle.is_finished(),
            "must not resolve on a single clear tick"
        );

        for _ in 0..2 {
            tokio::time::advance(Duration::from_millis(config.resume_poll_interval_ms)).await;
        }
        handle.await.unwrap();
        assert_eq!(
            sessions.lock().await.get("s1").unwrap().state,
            State::Working
        );
    }

    #[tokio::test(start_paused = true)]
    async fn resolution_aborts_sibling_idle_timer() {
        let config = config_with_interval(100);
        let target = WindowTarget::Id("1".to_string());
        let fake = Arc::new(FakeKittyClient::new(vec!["cleared", "cleared"]));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let sessions: Arc<Mutex<SessionTable>> = Arc::new(Mutex::new(SessionTable::new()));

        // Simulate a Waiting session: both timers armed.
        let idle_handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(999)).await;
        });
        let idle_abort = idle_handle.abort_handle();
        sessions.lock().await.insert(
            "s1".to_string(),
            Session {
                state: State::Waiting,
                idle_timer: Some(idle_handle),
                resume_watch: None,
            },
        );

        let handle = spawn(
            "s1".to_string(),
            target.clone(),
            sessions.clone(),
            client,
            config.clone(),
        );
        for _ in 0..2 {
            tokio::time::advance(Duration::from_millis(config.resume_poll_interval_ms)).await;
        }
        handle.await.unwrap();

        tokio::task::yield_now().await;
        assert!(idle_abort.is_finished());
    }
}
