use std::collections::{HashMap, HashSet};

use tokio::task::JoinHandle;

use crate::state::State;

#[derive(Debug)]
pub struct Session {
    pub state: State,
    /// Pending idle-timeout task (armed for Done/Waiting), if any.
    pub idle_timer: Option<JoinHandle<()>>,
    /// Pending resume-detection screen-scrape task (armed for
    /// Permission/Waiting), if any.
    pub resume_watch: Option<JoinHandle<()>>,
    /// agent_ids of currently-running background subagents (Task tool),
    /// tracked independently of `state`: see `State::AgentWorking`.
    /// Non-empty overlays the tab's icon/color regardless of what
    /// `state` is; membership is by agent_id, so a `SubagentStop` for an
    /// id never seen in a `SubagentStart` (Claude Code's own internal
    /// background-shell-command bookkeeping fires this, not just real
    /// subagents) is naturally a harmless no-op removal.
    pub active_agents: HashSet<String>,
}

/// In-memory only, keyed by session_id. No persistence: every hook event
/// carries full session context, so a crashed daemon self-heals from the
/// next hook fire rather than needing to reload state from disk.
pub type SessionTable = HashMap<String, Session>;
