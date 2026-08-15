use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::Config;
use crate::kitty::{KittyClient, WindowTarget};
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
                Err(_) => continue, // transient IPC failure — try again next tick
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
            let _ = client.set_tab_title(&target, &config.title_for(State::Working));
            let _ = client.set_tab_color(&target, &config.color_for(State::Working));
            return;
        }

        eprintln!(
            "kitty-claude-notifier: resume-watch for session {session_id} timed out \
             after {MAX_WAIT:?} without detecting resolution; last screen text:\n{last_text}"
        );
    })
}
