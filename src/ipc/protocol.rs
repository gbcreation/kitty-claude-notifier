use serde::{Deserialize, Serialize};

use crate::kitty::WindowTarget;
use crate::state::State;

/// One NDJSON line sent from a `hook` invocation to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMessage {
    pub session_id: Option<String>,
    pub target: WindowTarget,
    pub kind: MessageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageKind {
    SetState(State),
    Cleanup,
}
