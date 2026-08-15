use std::env;

use serde::{Deserialize, Serialize};

/// Which Kitty window/tab a command should target: mirrors the bash tool's
/// `--self`-when-available, `--match pid:$PPID`-otherwise fallback.
///
/// Resolved on the hook side (where KITTY_WINDOW_ID/PPID are meaningful) and
/// carried as-is over IPC to the daemon, whose own process tree has no
/// relationship to the terminal that fired the hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowTarget {
    Id(String),
    Pid(u32),
}

impl WindowTarget {
    /// Resolve the target the same way the invoking hook process sees it:
    /// prefer KITTY_WINDOW_ID, falling back to the parent process's PID
    /// (the shell running inside the target Kitty window).
    pub fn from_env() -> Self {
        match env::var("KITTY_WINDOW_ID") {
            Ok(id) if !id.is_empty() => WindowTarget::Id(id),
            _ => WindowTarget::Pid(nix::unistd::getppid().as_raw() as u32),
        }
    }

    pub fn match_expr(&self) -> String {
        match self {
            WindowTarget::Id(id) => format!("id:{id}"),
            WindowTarget::Pid(pid) => format!("pid:{pid}"),
        }
    }
}
