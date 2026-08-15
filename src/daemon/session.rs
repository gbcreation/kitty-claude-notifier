use std::collections::HashMap;

use tokio::task::JoinHandle;

use crate::kitty::WindowTarget;
use crate::state::State;

#[derive(Debug)]
pub struct Session {
    pub state: State,
    pub target: WindowTarget,
    pub transcript_path: Option<String>,
    /// Pending idle-timeout task (armed for Done/Waiting), if any.
    pub idle_timer: Option<JoinHandle<()>>,
    /// Pending resume-detection screen-scrape task (armed for
    /// Permission/Waiting), if any.
    pub resume_watch: Option<JoinHandle<()>>,
}

/// In-memory only, keyed by session_id. No persistence: every hook event
/// carries full session context, so a crashed daemon self-heals from the
/// next hook fire rather than needing to reload state from disk.
pub type SessionTable = HashMap<String, Session>;
