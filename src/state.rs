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
}

impl State {
    pub fn default_title(&self) -> &'static str {
        match self {
            State::Working => "⚡ Work",
            State::Permission => "⛔ Perm",
            State::Waiting => "⏳ Wait",
            State::Done => "✅ Done",
            State::Idle => "💤 Idle",
            State::Error => "❌ Error",
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
        };
        write!(f, "{s}")
    }
}
