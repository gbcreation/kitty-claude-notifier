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
    /// A background subagent (Task tool) was spawned. `agent_id`
    /// correlates with the matching `AgentStop` that ends it.
    AgentStart {
        agent_id: String,
    },
    /// A background subagent finished. A stop for an `agent_id` that
    /// was never started (e.g. Claude Code's own internal
    /// background-shell-command bookkeeping) is a no-op.
    AgentStop {
        agent_id: String,
    },
}
