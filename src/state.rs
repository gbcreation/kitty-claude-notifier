use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Mirrors claude-notifier's locked state model, minus `researching`/`normal`
/// (neither has a hook trigger, so lean v1 doesn't need them).
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
        };
        write!(f, "{s}")
    }
}
