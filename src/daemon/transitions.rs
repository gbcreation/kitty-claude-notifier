use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::config::Config;
use crate::ipc::protocol::{HookMessage, MessageKind};
use crate::kitty::{self, KittyClient};
use crate::sound::{self, Sound};
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
            let is_focused = kitty::apply(client.as_ref(), &msg.target, &icon, &active, &inactive);
            if let Some(session_id) = msg.session_id {
                let mut table = sessions.lock().await;
                // Any prior timers belong to a state this message
                // supersedes — abort them so neither can fire late.
                let old_state = table
                    .remove(&session_id)
                    .inspect(abort_timers)
                    .map(|s| s.state);

                if let Some(sound) = sound_for_transition(config.sound_enabled, old_state, state) {
                    // A tab you're already looking at doesn't need an
                    // audio nudge too — unless sound_play_when_focused
                    // opts back into it, in which case skip the focus
                    // check (and the fetch behind it) entirely.
                    // `apply()` above already fetched focus state unless
                    // a `text` icon override skipped that call entirely —
                    // in which case, fetch it now, failing open (assume
                    // unfocused) so a Kitty hiccup never silently
                    // swallows a real notification.
                    let should_play = config.sound_play_when_focused
                        || !is_focused.unwrap_or_else(|| {
                            client
                                .get_tab_info(&msg.target)
                                .map(|info| info.is_focused)
                                .unwrap_or(false)
                        });
                    if should_play {
                        let path = match sound {
                            Sound::Request => &config.sounds.request,
                            Sound::Done => &config.sounds.done,
                        };
                        sound::play(sound, path.clone().map(PathBuf::from));
                    }
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

/// Pure decision logic for which sound (if any) a state transition should
/// play — no I/O, so it's directly testable without needing to observe
/// real audio playback (which is a no-op in test builds regardless).
/// Plays nothing if sound is disabled, or if this isn't a genuine
/// transition (the new state matches the one already recorded).
fn sound_for_transition(
    sound_enabled: bool,
    old_state: Option<State>,
    new_state: State,
) -> Option<Sound> {
    if !sound_enabled || old_state == Some(new_state) {
        return None;
    }
    match new_state {
        State::Permission | State::Waiting => Some(Sound::Request),
        State::Done => Some(Sound::Done),
        State::Working | State::Idle | State::Error => None,
    }
}

#[cfg(test)]
mod sound_tests {
    use super::*;

    #[test]
    fn disabled_plays_nothing_regardless_of_transition() {
        assert_eq!(sound_for_transition(false, None, State::Permission), None);
        assert_eq!(
            sound_for_transition(false, Some(State::Idle), State::Done),
            None
        );
    }

    #[test]
    fn permission_and_waiting_play_request() {
        assert_eq!(
            sound_for_transition(true, None, State::Permission),
            Some(Sound::Request)
        );
        assert_eq!(
            sound_for_transition(true, Some(State::Working), State::Waiting),
            Some(Sound::Request)
        );
    }

    #[test]
    fn done_plays_done() {
        assert_eq!(
            sound_for_transition(true, Some(State::Working), State::Done),
            Some(Sound::Done)
        );
    }

    #[test]
    fn working_idle_error_play_nothing() {
        assert_eq!(sound_for_transition(true, None, State::Working), None);
        assert_eq!(sound_for_transition(true, None, State::Idle), None);
        assert_eq!(sound_for_transition(true, None, State::Error), None);
    }

    #[test]
    fn repeating_the_same_state_plays_nothing() {
        // Guards against duplicate hook firings re-triggering the sound.
        assert_eq!(
            sound_for_transition(true, Some(State::Permission), State::Permission),
            None
        );
        assert_eq!(
            sound_for_transition(true, Some(State::Done), State::Done),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IconSpec;
    use crate::kitty::WindowTarget;
    use crate::kitty::fake::{Call, FakeKittyClient};
    use std::collections::HashMap;

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

    fn count_get_tab_info_calls(fake: &FakeKittyClient) -> usize {
        fake.calls()
            .iter()
            .filter(|c| matches!(c, Call::GetTabInfo(_)))
            .count()
    }

    /// When no icon `text` override is configured, `kitty::apply()` already
    /// fetches focus while fetching the live title — the sound-suppression
    /// check must reuse that instead of fetching it again.
    #[tokio::test]
    async fn sound_reuses_focus_apply_already_fetched() {
        let sessions = Arc::new(Mutex::new(SessionTable::new()));
        let fake = Arc::new(FakeKittyClient::new(vec![]).with_focused(true));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let config = Arc::new(Config {
            sound_enabled: true,
            ..Config::default()
        });

        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        assert_eq!(count_get_tab_info_calls(&fake), 1);
    }

    /// When a `text` override is configured for this state, `apply()` skips
    /// fetching the live title entirely — and with it, focus state. The
    /// sound-suppression check must fall back to fetching focus on its own
    /// in that case rather than silently assuming a value.
    #[tokio::test]
    async fn sound_fetches_focus_separately_when_apply_skipped_it() {
        let sessions = Arc::new(Mutex::new(SessionTable::new()));
        let fake = Arc::new(FakeKittyClient::new(vec![]).with_focused(false));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let mut icons = HashMap::new();
        icons.insert(
            "permission".to_string(),
            IconSpec {
                glyph: "▲".to_string(),
                color: "#ffffff".to_string(),
                text: Some("NEEDS APPROVAL".to_string()),
            },
        );
        let config = Arc::new(Config {
            sound_enabled: true,
            icons,
            ..Config::default()
        });

        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        assert_eq!(
            count_get_tab_info_calls(&fake),
            1,
            "must fall back to fetching focus when apply() skipped it"
        );
    }

    /// sound_play_when_focused=true must skip the focus check (and the
    /// fetch behind it) entirely, not just play through a `false` focus
    /// result — the point is not to care about focus at all.
    #[tokio::test]
    async fn sound_play_when_focused_skips_the_focus_fetch_entirely() {
        let sessions = Arc::new(Mutex::new(SessionTable::new()));
        let fake = Arc::new(FakeKittyClient::new(vec![]).with_focused(true));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let mut icons = HashMap::new();
        icons.insert(
            "permission".to_string(),
            IconSpec {
                glyph: "▲".to_string(),
                color: "#ffffff".to_string(),
                text: Some("NEEDS APPROVAL".to_string()),
            },
        );
        let config = Arc::new(Config {
            sound_enabled: true,
            sound_play_when_focused: true,
            icons,
            ..Config::default()
        });

        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        assert_eq!(
            count_get_tab_info_calls(&fake),
            0,
            "sound_play_when_focused must not need to know the actual focus state"
        );
    }

    /// No sound-eligible transition (sound disabled, or the state isn't
    /// Permission/Waiting/Done) must never trigger even the fallback focus
    /// fetch — there's nothing to suppress.
    #[tokio::test]
    async fn no_focus_fetch_at_all_when_sound_disabled() {
        let sessions = Arc::new(Mutex::new(SessionTable::new()));
        let fake = Arc::new(FakeKittyClient::new(vec![]));
        let client: Arc<dyn KittyClient + Send + Sync> = fake.clone();
        let mut icons = HashMap::new();
        icons.insert(
            "permission".to_string(),
            IconSpec {
                glyph: "▲".to_string(),
                color: "#ffffff".to_string(),
                text: Some("NEEDS APPROVAL".to_string()),
            },
        );
        let config = Arc::new(Config {
            sound_enabled: false,
            icons,
            ..Config::default()
        });

        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        assert_eq!(count_get_tab_info_calls(&fake), 0);
    }
}
