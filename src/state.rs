use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Mirrors claude-notifier's locked state model, minus `researching`/`normal`
/// (neither has a hook trigger, so lean v1 doesn't need them).
///
/// `AgentWorking` is different from every other variant here: it's never
/// assigned to a `Session.state` and never participates in a normal
/// transition. It exists purely as an icon/color lookup key for the
/// background-agent overlay (see `visual()` below and
/// `daemon::transitions`'s `MessageKind::AgentStart`/`AgentStop`
/// handling), which can be active on top of *any* real state without
/// changing what that real state actually is underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Working,
    Permission,
    Waiting,
    Done,
    Idle,
    Error,
    Compacting,
    AgentWorking,
}

impl State {
    /// Default icon glyph prepended to the tab's existing title: a plain
    /// (non-emoji) Unicode symbol, since emoji ignore ANSI foreground color.
    pub fn default_icon_glyph(&self) -> &'static str {
        match self {
            State::Working => "●",
            State::Permission => "▲",
            State::Waiting => "◐",
            State::Done => "✓",
            State::Idle => "○",
            State::Error => "✕",
            State::Compacting => "▣",
            State::AgentWorking => "◑",
        }
    }

    /// Default icon color (hex), deliberately neutral/white rather than
    /// matching `default_color()`'s hue: the tab background is often the
    /// *same* saturated color, and a same-colored icon disappears into it.
    pub fn default_icon_color(&self) -> &'static str {
        match self {
            State::Working => "#ffffff",
            State::Permission => "#ffffff",
            State::Waiting => "#ffffff",
            State::Done => "#ffffff",
            State::Idle => "#888888",
            State::Error => "#ffffff",
            State::Compacting => "#ffffff",
            State::AgentWorking => "#ffffff",
        }
    }

    /// Default `active_bg`/`inactive_bg` value for `kitten @ set-tab-color`
    /// (the same color is used for both unless overridden in config.toml).
    pub fn default_color(&self) -> &'static str {
        match self {
            State::Working => "#b026ff",
            State::Permission => "#ff003c",
            State::Waiting => "#00ffd5",
            State::Done => "#00ffd5",
            State::Idle => "NONE",
            State::Error => "#ff6b00",
            State::Compacting => "#3fa7d6",
            State::AgentWorking => "#b026ff",
        }
    }

    /// The state whose icon/color should actually be painted on the tab.
    /// `AgentWorking` overlays whatever `self` (the real, hook-driven
    /// state) is, for as long as at least one background subagent is
    /// active; `self` never changes underneath it. Called at every
    /// point that repaints a tab (`transitions::apply`, `idle_timer`,
    /// `resume_watch`) so the overlay can never be silently dropped by
    /// one of them repainting with the real state directly.
    pub fn visual(self, has_active_agents: bool) -> State {
        if has_active_agents {
            State::AgentWorking
        } else {
            self
        }
    }
}

impl FromStr for State {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "working" => Ok(State::Working),
            "permission" => Ok(State::Permission),
            "waiting" => Ok(State::Waiting),
            "done" => Ok(State::Done),
            "idle" => Ok(State::Idle),
            "error" => Ok(State::Error),
            "compacting" => Ok(State::Compacting),
            "agent_working" => Ok(State::AgentWorking),
            other => Err(format!("unknown state: {other}")),
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            State::Working => "working",
            State::Permission => "permission",
            State::Waiting => "waiting",
            State::Done => "done",
            State::Idle => "idle",
            State::Error => "error",
            State::Compacting => "compacting",
            State::AgentWorking => "agent_working",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_is_real_state_when_no_agents_active() {
        assert_eq!(State::Working.visual(false), State::Working);
        assert_eq!(State::Done.visual(false), State::Done);
        assert_eq!(State::Permission.visual(false), State::Permission);
    }

    #[test]
    fn visual_overlays_agent_working_regardless_of_the_real_state() {
        assert_eq!(State::Working.visual(true), State::AgentWorking);
        assert_eq!(State::Done.visual(true), State::AgentWorking);
        assert_eq!(State::Waiting.visual(true), State::AgentWorking);
        assert_eq!(State::Idle.visual(true), State::AgentWorking);
    }
}
