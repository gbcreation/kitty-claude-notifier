use std::env;
use std::process::{Command, Output};

use anyhow::{Context, Result};

use super::{KittyClient, TabInfo, WindowTarget};

/// Real Kitty IPC: shells out to `kitten @`, always via the configured
/// KITTY_LISTEN_ON socket when present rather than relying on the calling
/// process having a controlling TTY (which hook subprocesses often lack).
pub struct ProcessKittyClient;

impl ProcessKittyClient {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        let mut cmd = Command::new("kitten");
        cmd.arg("@");
        if let Ok(listen_on) = env::var("KITTY_LISTEN_ON")
            && !listen_on.is_empty()
        {
            cmd.args(["--to", &listen_on]);
        }
        cmd.args(args);
        let output = cmd.output().context("failed to spawn kitten")?;
        if !output.status.success() {
            anyhow::bail!(
                "kitten @ {} exited with {}: {}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output)
    }
}

impl Default for ProcessKittyClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Extracts the filesystem path portion of a `KITTY_LISTEN_ON`-style
/// value (`unix:/tmp/kitty-1234` -> `/tmp/kitty-1234`). Pure, so it's
/// directly testable without touching the filesystem.
fn socket_path(listen_on: &str) -> &str {
    listen_on.strip_prefix("unix:").unwrap_or(listen_on)
}

/// Whether the `KITTY_LISTEN_ON` socket this process was started with is
/// still reachable. Its path is suffixed with Kitty's own PID
/// (`unix:/tmp/kitty-{pid}`), so once Kitty itself restarts (crash,
/// manual restart), the old path is gone for good and will never come
/// back under that PID; every `kitten @` call against it would otherwise
/// fail silently forever. Attempts a real connection rather than just
/// checking the file exists: a hard crash (SIGKILL, segfault) can leave a
/// stale socket file on disk with nothing listening on it, which would
/// still report "exists" but fail to connect. Returns `true` if
/// `KITTY_LISTEN_ON` isn't set at all, since there's nothing to check in
/// that case.
pub fn kitty_socket_reachable() -> bool {
    match env::var("KITTY_LISTEN_ON") {
        Ok(listen_on) if !listen_on.is_empty() => {
            kitty_socket_reachable_at(socket_path(&listen_on))
        }
        _ => true,
    }
}

/// The actual connection attempt, factored out so it's testable without
/// touching the real `KITTY_LISTEN_ON` environment variable.
fn kitty_socket_reachable_at(path: &str) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

impl KittyClient for ProcessKittyClient {
    fn set_tab_title(&self, target: &WindowTarget, title: &str) -> Result<()> {
        self.run(&["set-tab-title", "--match", &target.match_expr(), title])?;
        Ok(())
    }

    fn set_tab_color(
        &self,
        target: &WindowTarget,
        active_bg: &str,
        inactive_bg: &str,
    ) -> Result<()> {
        let active_spec = format!("active_bg={active_bg}");
        let inactive_spec = format!("inactive_bg={inactive_bg}");
        self.run(&[
            "set-tab-color",
            "--match",
            &target.match_expr(),
            &active_spec,
            &inactive_spec,
        ])?;
        Ok(())
    }

    fn get_tab_info(&self, target: &WindowTarget) -> Result<TabInfo> {
        let output = self.run(&["ls", "--match", &target.match_expr()])?;
        // `kitten @ ls` returns each matched window's full environment
        // variables (including secrets) and other sensitive process
        // details alongside the title. Parse just enough to extract the
        // title and focus state and drop the rest immediately. Never log
        // `output` or the parsed value.
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("failed to parse kitten @ ls output")?;
        let tab = parsed
            .as_array()
            .and_then(|os_windows| os_windows.first())
            .and_then(|w| w.get("tabs"))
            .and_then(|tabs| tabs.as_array())
            .and_then(|tabs| tabs.first())
            .context("no matching tab found in kitten @ ls output")?;
        let title = tab
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::to_string)
            .context("no title found in kitten @ ls output")?;
        let is_focused = tab
            .get("is_focused")
            .and_then(|f| f.as_bool())
            .unwrap_or(false);
        Ok(TabInfo { title, is_focused })
    }

    fn get_text(&self, target: &WindowTarget) -> Result<String> {
        let output = self.run(&["get-text", "--match", &target.match_expr()])?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_unix_prefix() {
        assert_eq!(socket_path("unix:/tmp/kitty-1234"), "/tmp/kitty-1234");
    }

    #[test]
    fn leaves_a_bare_path_unchanged() {
        assert_eq!(socket_path("/tmp/kitty-1234"), "/tmp/kitty-1234");
    }

    #[test]
    fn reachable_when_nothing_is_listening_there() {
        assert!(!kitty_socket_reachable_at(
            "/tmp/kitty-claude-notifier-test-definitely-does-not-exist"
        ));
    }

    #[test]
    fn reachable_when_a_real_listener_is_present() {
        let dir =
            std::env::temp_dir().join(format!("kitty-claude-notifier-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let listener = std::os::unix::net::UnixListener::bind(&dir).unwrap();
        assert!(kitty_socket_reachable_at(dir.to_str().unwrap()));
        drop(listener);
        let _ = std::fs::remove_file(&dir);
    }
}
