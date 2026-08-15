use std::collections::HashMap;

use crate::kitty::WindowTarget;
use crate::state::State;

#[derive(Debug, Clone)]
pub struct Session {
    pub state: State,
    pub target: WindowTarget,
    pub transcript_path: Option<String>,
}

/// In-memory only, keyed by session_id. No persistence: every hook event
/// carries full session context, so a crashed daemon self-heals from the
/// next hook fire rather than needing to reload state from disk.
pub type SessionTable = HashMap<String, Session>;
