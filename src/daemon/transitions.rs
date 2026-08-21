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
/// session_id; the tab update still happens, matching the bash tool's
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
                let old_state = table.get(&session_id).map(|s| s.state);

                if let Some(sound) = sound_for_transition(
                    config.sound_enabled,
                    &config.sound_events,
                    old_state,
                    state,
                ) {
                    // A tab you're already looking at doesn't need an
                    // audio nudge too, unless sound_play_when_focused
                    // opts back into it, in which case skip the focus
                    // check (and the fetch behind it) entirely.
                    // `apply()` above already fetched focus state unless
                    // a `text` icon override skipped that call entirely.
                    // In that case, fetch it now, failing open (assume
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

                if old_state == Some(state) && state != State::Permission {
                    // A repeat of the state the session is already in
                    // (e.g. Claude Code re-firing idle_prompt as a
                    // periodic nudge while Waiting). Leave the existing
                    // idle_timer/resume_watch running untouched: resetting
                    // them here would let a fast-enough repeat cadence
                    // perpetually postpone idle_timer, keeping the tab
                    // stuck instead of ever reaching Idle.
                    //
                    // Permission is deliberately excluded from this: its
                    // resolution relies entirely on resume_watch staying
                    // alive, and that task can hit its own MAX_WAIT safety
                    // timeout and die silently while Claude Code keeps
                    // sending repeated permission_prompt notifications for
                    // a prompt that's still genuinely open. Falling
                    // through below to restart resume_watch on every
                    // repeat means it can never die while Claude Code
                    // keeps asking, at the cost of discarding whatever
                    // poll progress the old one had made (at most a
                    // couple of poll intervals' delay).
                    tracing::info!(%session_id, ?state, "session state repeated, timers untouched");
                    return;
                }

                // A genuine transition, or a repeated Permission message
                // (see above): any prior timers belong to a state this
                // message supersedes, so abort them so neither can fire
                // late.
                if let Some(old) = table.remove(&session_id) {
                    abort_timers(&old);
                }

                let idle_timer = matches!(state, State::Done | State::Waiting | State::Compacting)
                    .then(|| {
                        idle_timer::spawn(
                            session_id.clone(),
                            msg.target.clone(),
                            Duration::from_secs(config.idle_timeout_secs),
                            sessions.clone(),
                            client.clone(),
                            config.clone(),
                        )
                    });
                // Permission dialogs are answered via a menu selection
                // (arrow keys + enter), which fires no hook at all, so
                // screen-scraping is the only way to detect resolution.
                // Waiting is different: a typed reply fires a real
                // UserPromptSubmit hook, already mapped to Working, so it
                // resolves correctly without screen-scraping; watching for
                // Claude Code's own wording here was unreliable, since the
                // "waiting for input" box's content varies every time (see
                // CLAUDE.md's Known limitations section).
                let resume_watch = matches!(state, State::Permission).then(|| {
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
/// play. No I/O, so it's directly testable without needing to observe
/// real audio playback (which is a no-op in test builds regardless).
/// Plays nothing if sound is disabled, if `new_state` isn't in
/// `sound_events`, or if this isn't a genuine transition (the new state
/// matches the one already recorded).
fn sound_for_transition(
    sound_enabled: bool,
    sound_events: &[State],
    old_state: Option<State>,
    new_state: State,
) -> Option<Sound> {
    if !sound_enabled || old_state == Some(new_state) || !sound_events.contains(&new_state) {
        return None;
    }
    match new_state {
        State::Permission | State::Waiting => Some(Sound::Request),
        State::Done => Some(Sound::Done),
        State::Working | State::Idle | State::Error | State::Compacting => None,
    }
}

#[cfg(test)]
mod sound_tests {
    use super::*;

    fn all_events() -> Vec<State> {
        vec![State::Permission, State::Waiting, State::Done]
    }

    #[test]
    fn disabled_plays_nothing_regardless_of_transition() {
        assert_eq!(
            sound_for_transition(false, &all_events(), None, State::Permission),
            None
        );
        assert_eq!(
            sound_for_transition(false, &all_events(), Some(State::Idle), State::Done),
            None
        );
    }

    #[test]
    fn permission_and_waiting_play_request() {
        assert_eq!(
            sound_for_transition(true, &all_events(), None, State::Permission),
            Some(Sound::Request)
        );
        assert_eq!(
            sound_for_transition(true, &all_events(), Some(State::Working), State::Waiting),
            Some(Sound::Request)
        );
    }

    #[test]
    fn done_plays_done() {
        assert_eq!(
            sound_for_transition(true, &all_events(), Some(State::Working), State::Done),
            Some(Sound::Done)
        );
    }

    #[test]
    fn working_idle_error_play_nothing() {
        assert_eq!(
            sound_for_transition(true, &all_events(), None, State::Working),
            None
        );
        assert_eq!(
            sound_for_transition(true, &all_events(), None, State::Idle),
            None
        );
        assert_eq!(
            sound_for_transition(true, &all_events(), None, State::Error),
            None
        );
    }

    #[test]
    fn repeating_the_same_state_plays_nothing() {
        // Guards against duplicate hook firings re-triggering the sound.
        assert_eq!(
            sound_for_transition(
                true,
                &all_events(),
                Some(State::Permission),
                State::Permission
            ),
            None
        );
        assert_eq!(
            sound_for_transition(true, &all_events(), Some(State::Done), State::Done),
            None
        );
    }

    #[test]
    fn state_not_in_sound_events_plays_nothing() {
        // A user who only wants a sound for Done can list just that.
        let events = vec![State::Done];
        assert_eq!(
            sound_for_transition(true, &events, None, State::Permission),
            None
        );
        assert_eq!(
            sound_for_transition(true, &events, Some(State::Working), State::Waiting),
            None
        );
        assert_eq!(
            sound_for_transition(true, &events, Some(State::Working), State::Done),
            Some(Sound::Done)
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
    async fn repeated_permission_message_restarts_resume_watch() {
        // Regression: unlike other states, a repeated Permission message
        // must restart resume_watch fresh rather than leaving it
        // untouched. It's the only thing watching for resolution, and
        // could otherwise die silently at its own MAX_WAIT safety
        // timeout while Claude Code keeps sending repeated
        // permission_prompt notifications for a prompt that's still
        // genuinely open, leaving the tab stuck forever with nothing
        // polling it.
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        let original_resume = {
            let table = sessions.lock().await;
            table
                .get("s1")
                .unwrap()
                .resume_watch
                .as_ref()
                .unwrap()
                .abort_handle()
        };
        assert!(!original_resume.is_finished());

        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        tokio::task::yield_now().await;
        assert!(
            original_resume.is_finished(),
            "repeated Permission message must abort the old resume_watch and start a fresh one"
        );

        let table = sessions.lock().await;
        let session = table.get("s1").unwrap();
        assert_eq!(session.state, State::Permission);
        assert!(session.resume_watch.is_some());
        assert!(session.idle_timer.is_none());
    }

    #[tokio::test]
    async fn waiting_arms_idle_timer_but_not_resume_watch() {
        // Waiting resolves via a real UserPromptSubmit hook once you
        // reply, or idle_timer if you don't; resume_watch's marker
        // matching only makes sense for Permission (see CLAUDE.md's
        // Known limitations section).
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
        assert!(session.resume_watch.is_none());
        assert!(session.idle_timer.is_some());
    }

    #[tokio::test]
    async fn compacting_arms_idle_timer_but_not_resume_watch() {
        // idle_timer here is a safety net in case PostCompact never fires
        // (e.g. the process was interrupted mid-compaction); resume_watch
        // makes no sense since there's no screen text to watch for.
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Compacting),
            &sessions,
            &client,
            &config,
        )
        .await;

        let table = sessions.lock().await;
        let session = table.get("s1").unwrap();
        assert!(session.resume_watch.is_none());
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
    async fn fresh_message_aborts_previous_idle_timer() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Waiting),
            &sessions,
            &client,
            &config,
        )
        .await;

        let old_idle = {
            let table = sessions.lock().await;
            table
                .get("s1")
                .unwrap()
                .idle_timer
                .as_ref()
                .unwrap()
                .abort_handle()
        };
        assert!(!old_idle.is_finished());

        // A genuine transition (different state) for the same session
        // should retire the superseded timer.
        apply(
            set_state_msg("s1", State::Done),
            &sessions,
            &client,
            &config,
        )
        .await;

        // Give the aborted task a moment to actually stop.
        tokio::task::yield_now().await;
        assert!(old_idle.is_finished());
    }

    #[tokio::test]
    async fn fresh_message_aborts_previous_resume_watch() {
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Permission),
            &sessions,
            &client,
            &config,
        )
        .await;

        let old_resume = {
            let table = sessions.lock().await;
            table
                .get("s1")
                .unwrap()
                .resume_watch
                .as_ref()
                .unwrap()
                .abort_handle()
        };
        assert!(!old_resume.is_finished());

        apply(
            set_state_msg("s1", State::Working),
            &sessions,
            &client,
            &config,
        )
        .await;

        tokio::task::yield_now().await;
        assert!(old_resume.is_finished());
    }

    #[tokio::test]
    async fn repeated_same_state_message_leaves_idle_timer_untouched() {
        // Regression: Claude Code appears to re-fire idle_prompt
        // periodically while Waiting. If a repeat reset idle_timer's
        // countdown, a session could stay stuck on Waiting forever as
        // long as the nudges arrived faster than the timeout.
        let (sessions, fake, config) = harness();
        let client = as_trait_object(&fake);
        apply(
            set_state_msg("s1", State::Waiting),
            &sessions,
            &client,
            &config,
        )
        .await;

        let original_idle = {
            let table = sessions.lock().await;
            table
                .get("s1")
                .unwrap()
                .idle_timer
                .as_ref()
                .unwrap()
                .abort_handle()
        };

        apply(
            set_state_msg("s1", State::Waiting),
            &sessions,
            &client,
            &config,
        )
        .await;

        tokio::task::yield_now().await;
        assert!(
            !original_idle.is_finished(),
            "repeated message must not abort/replace the existing idle_timer"
        );
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
    /// fetches focus while fetching the live title. The sound-suppression
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
    /// fetching the live title entirely, and with it, focus state. The
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
    /// result. The point is not to care about focus at all.
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
    /// fetch, since there's nothing to suppress.
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
