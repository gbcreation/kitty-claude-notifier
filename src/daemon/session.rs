use std::collections::HashMap;

use tokio::task::JoinHandle;

use crate::kitty::WindowTarget;
use crate::state::State;

#[derive(Debug)]
pub struct Session {
    pub state: State,
    pub target: WindowTarget,
    pub transcript_path: Option<String>,
    /// Handle to this session's pending idle-timeout task, if `state` is
    /// `Done`/`Waiting`. Aborted whenever a fresher message supersedes it,
    /// so a stale timer can never fire after the state has moved on.
    pub idle_timer: Option<JoinHandle<()>>,
}

/// In-memory only, keyed by session_id. No persistence: every hook event
/// carries full session context, so a crashed daemon self-heals from the
/// next hook fire rather than needing to reload state from disk.
pub type SessionTable = HashMap<String, Session>;
