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

    /// Match expression for *window*-scoped commands (`kitten @ ls`,
    /// `get-text`): for these, `id:` unambiguously means window id.
    pub fn match_expr(&self) -> String {
        match self {
            WindowTarget::Id(id) => format!("id:{id}"),
            WindowTarget::Pid(pid) => format!("pid:{pid}"),
        }
    }

    /// Match expression for *tab*-scoped commands (`set-tab-title`,
    /// `set-tab-color`). These do **not** treat `id:` the same way:
    /// per `kitten @ set-tab-color --help`, `id:` matches a *tab's own*
    /// id first, only falling back to "the tab containing a window with
    /// this id" if no tab has that numeric id. Since tab ids and window
    /// ids are drawn from the same counter and can therefore collide
    /// (confirmed live: a window id numerically equal to an unrelated
    /// tab's id caused that unrelated tab to be recolored instead), tab
    /// commands must instead use `window_id:`, which always means "the
    /// tab containing the window with this id" with no such ambiguity.
    /// `pid:` has no such fallback order printed in the docs since a tab
    /// itself has no pid of its own, so it already means "via a
    /// contained window" unambiguously for both command kinds.
    pub fn tab_match_expr(&self) -> String {
        match self {
            WindowTarget::Id(id) => format!("window_id:{id}"),
            WindowTarget::Pid(pid) => format!("pid:{pid}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_scoped_match_expr_uses_bare_id() {
        assert_eq!(WindowTarget::Id("42".to_string()).match_expr(), "id:42");
        assert_eq!(WindowTarget::Pid(42).match_expr(), "pid:42");
    }

    #[test]
    fn tab_scoped_match_expr_uses_window_id_to_avoid_tab_id_collisions() {
        assert_eq!(
            WindowTarget::Id("42".to_string()).tab_match_expr(),
            "window_id:42"
        );
        assert_eq!(WindowTarget::Pid(42).tab_match_expr(), "pid:42");
    }
}
